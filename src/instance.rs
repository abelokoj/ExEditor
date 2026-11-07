//! Windows file-open handoff, scoped to the current user and application-data directory.

#[cfg(windows)]
pub(crate) use windows::{Instance, Startup, start};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{Instance, Startup, start};

#[cfg(windows)]
mod windows {
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::fs::{self, File, TryLockError};
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::{Path, PathBuf};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail, ensure};
    use windows_sys::Win32::Foundation::{
        ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE,
        FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
        OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT, WriteFile,
    };
    use windows_sys::Win32::System::Com::CoCreateGuid;
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
        SetNamedPipeHandleState,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    use windows_sys::core::GUID;

    // Batches exceed normal Windows command-line capacity but remain bounded at the IPC boundary.
    const MAX_PAYLOAD: usize = 1024 * 1024;
    const MAX_PATHS: usize = 1024;
    const MAX_PATH_UNITS: usize = 32767;
    const MAX_QUEUED_BATCHES: usize = 128;
    const MAX_QUEUED_BYTES: usize = 8 * MAX_PAYLOAD;
    const MAX_RECENT_IDS: usize = 4096;
    const MAGIC: &[u8; 8] = b"EXEDIT01";
    const ACCEPTED: u8 = 1;
    const RETRY: u8 = 2;
    const INVALID: u8 = 3;
    const RETRY_DELAY: Duration = Duration::from_millis(25);
    const IO_TIMEOUT: Duration = Duration::from_secs(2);
    const START_TIMEOUT: Duration = Duration::from_secs(15);

    #[derive(Debug)]
    pub(crate) enum Startup {
        Primary(Instance),
        Forwarded,
    }

    /// Keep this guard alive until the application has finished running.
    #[derive(Debug)]
    pub(crate) struct Instance {
        shared: Arc<Shared>,
    }

    #[derive(Debug)]
    struct Shared {
        stopped: AtomicBool,
        queue: Mutex<Queue>,
    }

    #[derive(Debug)]
    struct Queue {
        accepting: bool,
        batches: Vec<Vec<PathBuf>>,
        bytes: usize,
        recent_ids: VecDeque<[u8; 16]>,
    }

    impl Queue {
        fn offer(&mut self, id: [u8; 16], paths: Vec<PathBuf>, bytes: usize) -> u8 {
            if self.recent_ids.contains(&id) {
                return ACCEPTED;
            }
            if !self.accepting
                || self.batches.len() >= MAX_QUEUED_BATCHES
                || self.bytes + bytes > MAX_QUEUED_BYTES
            {
                return RETRY;
            }
            self.batches.push(paths);
            self.bytes += bytes;
            self.recent_ids.push_back(id);
            if self.recent_ids.len() > MAX_RECENT_IDS {
                self.recent_ids.pop_front();
            }
            ACCEPTED
        }
        fn take(&mut self) -> Vec<Vec<PathBuf>> {
            self.bytes = 0;
            std::mem::take(&mut self.batches)
        }
    }

    impl Instance {
        /// Removes queued batches; an empty batch requests application activation.
        pub(crate) fn take_requests(&self) -> Vec<Vec<PathBuf>> {
            self.shared
                .queue
                .lock()
                .expect("instance queue poisoned")
                .take()
        }
        /// Stops accepting new batches and returns requests that must be handled before shutdown.
        pub(crate) fn stop_accepting(&self) -> Vec<Vec<PathBuf>> {
            let mut queue = self.shared.queue.lock().expect("instance queue poisoned");
            queue.accepting = false;
            queue.take()
        }
        /// Reopens the handoff after the application has deferred shutdown.
        pub(crate) fn resume_accepting(&self) {
            self.shared
                .queue
                .lock()
                .expect("instance queue poisoned")
                .accepting = true;
        }
    }

    impl Drop for Instance {
        fn drop(&mut self) {
            // The worker retains the election lock until its pipe is closed.
            self.shared.stopped.store(true, Ordering::Release);
        }
    }

    /// Elects a primary or forwards one batch, resolving paths in the launcher's working directory.
    ///
    /// Errors are visible to the launcher; an unacknowledged request must never be silently discarded.
    pub(crate) fn start(paths: &[PathBuf]) -> Result<Startup> {
        start_in(&crate::document::data_dir()?, paths)
    }

    fn start_in(directory: &Path, paths: &[PathBuf]) -> Result<Startup> {
        let payload = encode(paths, request_id()?)?;
        fs::create_dir_all(directory).context("Creating the instance handoff directory")?;
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(directory.join("instance.lock"))
            .context("Opening the instance election lock")?;
        // SAFETY: GetCurrentProcess returns a valid borrowed process pseudo-handle.
        let sid = process_sid(unsafe { GetCurrentProcess() })?;
        let name = pipe_name(&lock, &sid)?;
        let deadline = Instant::now() + START_TIMEOUT;
        let mut last_error = None;
        loop {
            match lock.try_lock() {
                Ok(()) => return make_primary(lock, &name, &sid).map(Startup::Primary),
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(error)) => {
                    return Err(error).context("Acquiring the instance election lock");
                }
            }
            match forward(&name, &sid, &payload) {
                Ok(ACCEPTED) => return Ok(Startup::Forwarded),
                Ok(INVALID) => bail!("The running ExEditor rejected the file-open request"),
                Ok(_) => {}
                Err(error) => last_error = Some(error),
            }
            if Instant::now() >= deadline {
                let detail = last_error
                    .map(|error| format!(": {error:#}"))
                    .unwrap_or_default();
                bail!(
                    "The running ExEditor did not accept these files{detail}. Please retry opening them."
                );
            }
            thread::sleep(RETRY_DELAY);
        }
    }

    fn pipe_name(lock: &File, sid: &str) -> Result<Vec<u16>> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: The borrowed file handle is live and info is writable for the call.
        if unsafe { GetFileInformationByHandle(lock.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error()).context("Identifying the instance directory");
        }
        // Filesystem identity makes junctions and case aliases share the same election and endpoint.
        Ok(format!(
            r"\\.\pipe\ExEditor-{sid}-{:08x}-{:08x}{:08x}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        )
        .encode_utf16()
        .chain(Some(0))
        .collect())
    }

    fn make_primary(lock: File, name: &[u16], sid: &str) -> Result<Instance> {
        let security = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut descriptor = ptr::null_mut();
        // SAFETY: security is NUL-terminated; descriptor receives a LocalFree-owned allocation.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                security.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("Securing the instance handoff");
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // SAFETY: name and security attributes remain live; the returned pipe handle is uniquely owned.
        let raw = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64,
                MAX_PAYLOAD as u32,
                0,
                &attributes,
            )
        };
        let pipe_error = (raw == INVALID_HANDLE_VALUE).then(io::Error::last_os_error);
        // SAFETY: descriptor was allocated by the security conversion above and is no longer needed.
        unsafe { LocalFree(descriptor) };
        if let Some(error) = pipe_error {
            return Err(error).context("Creating the instance handoff pipe");
        }
        // SAFETY: CreateNamedPipeW returned a valid uniquely owned handle.
        let pipe = unsafe { OwnedHandle::from_raw_handle(raw) };
        let shared = Arc::new(Shared {
            stopped: AtomicBool::new(false),
            queue: Mutex::new(Queue {
                accepting: true,
                batches: Vec::new(),
                bytes: 0,
                recent_ids: VecDeque::new(),
            }),
        });
        let worker_shared = shared.clone();
        thread::Builder::new()
            .name("file-open-handoff".into())
            .spawn(move || serve(pipe, lock, &worker_shared))
            .context("Starting the instance handoff listener")?;
        Ok(Instance { shared })
    }

    fn serve(pipe: OwnedHandle, _lock: File, shared: &Shared) {
        let mut bytes = vec![0; MAX_PAYLOAD];
        while !shared.stopped.load(Ordering::Acquire) {
            let connected = overlapped(&pipe, &shared.stopped, IO_TIMEOUT, |overlap, _| {
                // SAFETY: pipe and overlap remain live until completion or cancellation.
                let result = unsafe { ConnectNamedPipe(pipe.as_raw_handle(), overlap) };
                if result == 0
                    && io::Error::last_os_error().raw_os_error()
                        == Some(ERROR_PIPE_CONNECTED as i32)
                {
                    1
                } else {
                    result
                }
            });
            if connected.is_ok() {
                let received = read(&pipe, &mut bytes, &shared.stopped);
                if let Ok(length) = received {
                    let acknowledgement = match decode(&bytes[..length]) {
                        Ok((id, paths)) => {
                            let mut queue = shared.queue.lock().expect("instance queue poisoned");
                            if shared.stopped.load(Ordering::Acquire) {
                                RETRY
                            } else {
                                queue.offer(id, paths, length)
                            }
                        }
                        Err(_) => INVALID,
                    };
                    if write(&pipe, &[acknowledgement], &shared.stopped).is_ok() {
                        // A receipt prevents DisconnectNamedPipe from discarding an unread acknowledgement.
                        let _ = read(&pipe, &mut [0], &shared.stopped);
                    }
                }
            }
            // SAFETY: The server owns this handle and all of its overlapped operations have completed.
            unsafe { DisconnectNamedPipe(pipe.as_raw_handle()) };
            if connected.is_err() {
                thread::sleep(RETRY_DELAY);
            }
        }
        // Close the pipe before releasing the lock so a new primary can create its endpoint.
        drop(pipe);
    }

    fn forward(name: &[u16], sid: &str, payload: &[u8]) -> Result<u8> {
        // SAFETY: name is NUL-terminated; a successful result is a uniquely owned overlapped handle.
        let raw = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(code) if code == ERROR_FILE_NOT_FOUND as i32
                || code == ERROR_PIPE_BUSY as i32 || code == ERROR_BROKEN_PIPE as i32)
            {
                return Ok(RETRY);
            }
            return Err(error).context("Connecting to the running ExEditor");
        }
        // SAFETY: CreateFileW returned a valid uniquely owned handle.
        let pipe = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut process_id = 0;
        // SAFETY: The pipe is live; process_id is writable.
        if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut process_id) } == 0 {
            return Err(io::Error::last_os_error()).context("Identifying the running ExEditor");
        }
        // SAFETY: Opening with query-only access neither mutates the process nor inherits its handle.
        let raw_process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        ensure!(
            !raw_process.is_null(),
            "Could not verify the running ExEditor's user"
        );
        // SAFETY: OpenProcess returned a valid uniquely owned handle.
        let process = unsafe { OwnedHandle::from_raw_handle(raw_process) };
        ensure!(
            process_sid(process.as_raw_handle())? == sid,
            "The instance handoff belongs to another user"
        );
        // SAFETY: The authenticated server PID is live; this only grants permission to activate its UI.
        unsafe { AllowSetForegroundWindow(process_id) };
        let mode = PIPE_READMODE_MESSAGE;
        // SAFETY: The live pipe is a message pipe and mode is valid for the duration of the call.
        if unsafe { SetNamedPipeHandleState(pipe.as_raw_handle(), &mode, ptr::null(), ptr::null()) }
            == 0
        {
            return Err(io::Error::last_os_error()).context("Configuring the instance handoff");
        }
        let stopped = AtomicBool::new(false);
        write(&pipe, payload, &stopped).context("Sending files to the running ExEditor")?;
        let mut acknowledgement = [0];
        ensure!(
            read(&pipe, &mut acknowledgement, &stopped)? == 1,
            "Missing instance handoff acknowledgement"
        );
        // The acknowledgement has been read, so a lost receipt cannot lose this request.
        let _ = write(&pipe, &[0], &stopped);
        ensure!(
            matches!(acknowledgement[0], ACCEPTED | RETRY | INVALID),
            "Invalid instance handoff acknowledgement"
        );
        Ok(acknowledgement[0])
    }

    fn read(pipe: &OwnedHandle, bytes: &mut [u8], stopped: &AtomicBool) -> io::Result<usize> {
        overlapped(pipe, stopped, IO_TIMEOUT, |overlap, transferred| {
            // SAFETY: bytes is exclusively borrowed until completion; overlap remains live throughout.
            let result = unsafe {
                ReadFile(
                    pipe.as_raw_handle(),
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    ptr::null_mut(),
                    overlap,
                )
            };
            if result == 0 {
                return result;
            }
            // SAFETY: A successful ReadFile completed this operation; retrieve its actual byte count.
            unsafe { GetOverlappedResult(pipe.as_raw_handle(), overlap, transferred, 0) }
        })
    }

    fn write(pipe: &OwnedHandle, bytes: &[u8], stopped: &AtomicBool) -> io::Result<()> {
        let length = overlapped(pipe, stopped, IO_TIMEOUT, |overlap, transferred| {
            // SAFETY: bytes and overlap remain live until completion or cancellation.
            let result = unsafe {
                WriteFile(
                    pipe.as_raw_handle(),
                    bytes.as_ptr(),
                    bytes.len() as u32,
                    ptr::null_mut(),
                    overlap,
                )
            };
            if result == 0 {
                return result;
            }
            // SAFETY: A successful WriteFile completed this operation; retrieve its actual byte count.
            unsafe { GetOverlappedResult(pipe.as_raw_handle(), overlap, transferred, 0) }
        })?;
        if length != bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "Incomplete instance handoff message",
            ));
        }
        Ok(())
    }

    fn overlapped(
        pipe: &OwnedHandle,
        stopped: &AtomicBool,
        timeout: Duration,
        operation: impl FnOnce(*mut OVERLAPPED, *mut u32) -> i32,
    ) -> io::Result<usize> {
        // SAFETY: An unnamed, non-inheritable manual-reset event has no borrowed inputs.
        let raw_event = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if raw_event.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateEventW returned a valid uniquely owned handle.
        let event = unsafe { OwnedHandle::from_raw_handle(raw_event) };
        let mut overlap = OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..OVERLAPPED::default()
        };
        let mut transferred = 0;
        if operation(&mut overlap, &mut transferred) != 0 {
            return Ok(transferred as usize);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(error);
        }
        let deadline = Instant::now() + timeout;
        let error = loop {
            if stopped.load(Ordering::Acquire) {
                break io::Error::new(io::ErrorKind::Interrupted, "Instance handoff stopped");
            }
            if Instant::now() >= deadline {
                break io::Error::new(io::ErrorKind::TimedOut, "Instance handoff timed out");
            }
            // SAFETY: event is live and owned until the operation has completed.
            match unsafe { WaitForSingleObject(event.as_raw_handle(), 25) } {
                WAIT_OBJECT_0 => {
                    // SAFETY: This is the same live pipe and OVERLAPPED submitted to the operation.
                    if unsafe {
                        GetOverlappedResult(pipe.as_raw_handle(), &overlap, &mut transferred, 0)
                    } == 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok(transferred as usize);
                }
                WAIT_TIMEOUT => {}
                _ => break io::Error::last_os_error(),
            }
        };
        // SAFETY: Cancel only our pending operation, then wait before its stack and buffer are released.
        unsafe {
            CancelIoEx(pipe.as_raw_handle(), &overlap);
            GetOverlappedResult(pipe.as_raw_handle(), &overlap, &mut transferred, 1);
        }
        Err(error)
    }

    fn process_sid(process: HANDLE) -> Result<String> {
        let mut raw_token = ptr::null_mut();
        // SAFETY: process is a live borrowed process handle; raw_token receives an owned query token.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw_token) } == 0 {
            return Err(io::Error::last_os_error()).context("Reading the instance user's identity");
        }
        // SAFETY: OpenProcessToken returned a valid uniquely owned token.
        let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
        let mut length = 0;
        // SAFETY: This query requests only the required size and supplies no output buffer.
        unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut length,
            )
        };
        ensure!(
            length as usize >= std::mem::size_of::<TOKEN_USER>(),
            "The user identity has an invalid size"
        );
        // usize storage gives TOKEN_USER its required pointer alignment.
        let mut storage = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: The aligned allocation has at least length bytes and remains live through SID conversion.
        if unsafe {
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                storage.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error()).context("Reading the instance user's identity");
        }
        // SAFETY: GetTokenInformation initialized an aligned TOKEN_USER in this live allocation.
        let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
        let mut raw_sid = ptr::null_mut();
        // SAFETY: user.User.Sid points inside the live token-information allocation.
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut raw_sid) } == 0 {
            return Err(io::Error::last_os_error())
                .context("Formatting the instance user's identity");
        }
        // SAFETY: A successful SID conversion returns a NUL-terminated UTF-16 allocation.
        let sid = unsafe {
            let mut length = 0;
            while *raw_sid.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(raw_sid, length))
        };
        // SAFETY: raw_sid was allocated by ConvertSidToStringSidW and has been copied above.
        unsafe { LocalFree(raw_sid.cast()) };
        Ok(sid)
    }

    fn request_id() -> Result<[u8; 16]> {
        let mut guid = GUID::default();
        // SAFETY: guid is writable; CoCreateGuid requires no COM initialization.
        ensure!(
            unsafe { CoCreateGuid(&mut guid) } >= 0,
            "Creating a file-open request identifier failed"
        );
        let mut id = [0; 16];
        id[..4].copy_from_slice(&guid.data1.to_le_bytes());
        id[4..6].copy_from_slice(&guid.data2.to_le_bytes());
        id[6..8].copy_from_slice(&guid.data3.to_le_bytes());
        id[8..].copy_from_slice(&guid.data4);
        Ok(id)
    }

    fn encode(paths: &[PathBuf], id: [u8; 16]) -> Result<Vec<u8>> {
        ensure!(paths.len() <= MAX_PATHS, "Too many files in one launch");
        let mut bytes = Vec::from(*MAGIC);
        bytes.extend_from_slice(&id);
        bytes.extend_from_slice(&(paths.len() as u32).to_le_bytes());
        for path in paths {
            ensure!(!path.as_os_str().is_empty(), "A file path is empty");
            let path = std::path::absolute(path).context("Resolving a file path before handoff")?;
            let units: Vec<_> = path.as_os_str().encode_wide().collect();
            ensure!(
                units.len() <= MAX_PATH_UNITS && !units.contains(&0),
                "A file path is invalid or too long"
            );
            ensure!(
                bytes.len() + 4 + units.len() * 2 <= MAX_PAYLOAD,
                "Too many file paths in one launch"
            );
            bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
            for unit in units {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
        }
        Ok(bytes)
    }

    fn decode(bytes: &[u8]) -> Result<([u8; 16], Vec<PathBuf>)> {
        ensure!(bytes.len() <= MAX_PAYLOAD, "Instance handoff is too large");
        let mut input = bytes;
        ensure!(
            take(&mut input, MAGIC.len())? == MAGIC,
            "Unknown instance handoff format"
        );
        let id = take(&mut input, 16)?
            .try_into()
            .context("Invalid request identifier")?;
        let count = word(&mut input)? as usize;
        ensure!(count <= MAX_PATHS, "Too many paths in an instance handoff");
        let mut paths = Vec::with_capacity(count);
        for _ in 0..count {
            let length = word(&mut input)? as usize;
            ensure!(
                length > 0 && length <= MAX_PATH_UNITS,
                "Invalid instance path length"
            );
            let units: Vec<_> = take(&mut input, length * 2)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            ensure!(
                !units.contains(&0),
                "Instance path contains a NUL character"
            );
            let path = PathBuf::from(OsString::from_wide(&units));
            ensure!(path.is_absolute(), "Instance path must be absolute");
            paths.push(path);
        }
        ensure!(input.is_empty(), "Trailing bytes in instance handoff");
        Ok((id, paths))
    }

    fn word(input: &mut &[u8]) -> Result<u32> {
        Ok(u32::from_le_bytes(
            take(input, 4)?
                .try_into()
                .context("Invalid instance word")?,
        ))
    }

    fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8]> {
        ensure!(input.len() >= length, "Truncated instance handoff");
        let (value, rest) = input.split_at(length);
        *input = rest;
        Ok(value)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn primary(directory: &Path) -> Instance {
            match start_in(directory, &[]).expect("starting isolated primary") {
                Startup::Primary(instance) => instance,
                Startup::Forwarded => panic!("isolated directory already has a primary"),
            }
        }

        #[test]
        fn paths_round_trip_without_losing_unicode_spaces_or_surrogates() {
            let raw = OsString::from_wide(&[67, 58, 92, 0xd800, 46, 116, 120, 116]);
            let paths = vec![
                PathBuf::from(r"C:\a folder\文書 📝.txt"),
                PathBuf::from(raw),
            ];
            let payload = encode(&paths, [7; 16]).expect("encoding valid Windows paths");
            let (id, decoded) = decode(&payload).expect("decoding valid Windows paths");
            assert_eq!(id, [7; 16], "request identity must survive encoding");
            assert_eq!(
                decoded, paths,
                "Windows paths must retain every UTF-16 unit"
            );
            let relative = PathBuf::from("a folder/文書.txt");
            assert_eq!(
                decode(&encode(std::slice::from_ref(&relative), [1; 16]).unwrap())
                    .unwrap()
                    .1,
                vec![std::path::absolute(relative).unwrap()],
                "resolve relative paths in the launcher"
            );
        }

        #[test]
        fn invalid_or_unbounded_messages_are_rejected() {
            let payload = encode(&[PathBuf::from(r"C:\test.txt")], [1; 16]).unwrap();
            for length in 0..payload.len() {
                assert!(
                    decode(&payload[..length]).is_err(),
                    "accepted a truncated payload of {length} bytes"
                );
            }
            let mut trailing = payload.clone();
            trailing.push(0);
            assert!(decode(&trailing).is_err(), "trailing data must be rejected");
            let mut excessive_count = payload.clone();
            excessive_count[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(
                decode(&excessive_count).is_err(),
                "unbounded path counts must be rejected"
            );
            let mut nul_path = payload.clone();
            nul_path[32..34].fill(0);
            assert!(decode(&nul_path).is_err(), "embedded NUL must be rejected");
            let mut relative_path = payload;
            relative_path[34..36].copy_from_slice(&(b'x' as u16).to_le_bytes());
            assert!(
                decode(&relative_path).is_err(),
                "relative paths must be rejected"
            );
            assert!(
                decode(&vec![0; MAX_PAYLOAD + 1]).is_err(),
                "oversized IPC payload must be rejected"
            );
            assert!(
                encode(&[PathBuf::new()], [0; 16]).is_err(),
                "empty path must be rejected"
            );
        }

        #[test]
        fn parallel_launches_forward_complete_batches_and_activation() {
            let directory = tempfile::tempdir().unwrap();
            let primary = primary(directory.path());
            thread::scope(|scope| {
                let mut clients = Vec::new();
                for index in 0..8 {
                    let directory = directory.path();
                    clients.push(scope.spawn(move || {
                        let paths = vec![PathBuf::from(format!(r"C:\a folder\{index} 文書.txt"))];
                        assert!(
                            matches!(start_in(directory, &paths).unwrap(), Startup::Forwarded),
                            "secondary launch {index} must forward"
                        );
                    }));
                }
                for client in clients {
                    client.join().unwrap();
                }
            });
            assert!(
                matches!(start_in(directory.path(), &[]).unwrap(), Startup::Forwarded),
                "empty launch must activate"
            );
            let batches = primary.take_requests();
            assert_eq!(
                batches.len(),
                9,
                "all concurrent batches and activation must arrive once"
            );
            assert_eq!(
                batches.iter().filter(|batch| batch.is_empty()).count(),
                1,
                "activation remains a separate batch"
            );
            let mut actual: Vec<_> = batches.into_iter().flatten().collect();
            actual.sort();
            let expected: Vec<_> = (0..8)
                .map(|index| PathBuf::from(format!(r"C:\a folder\{index} 文書.txt")))
                .collect();
            assert_eq!(
                actual, expected,
                "concurrent handoff must preserve complete file names"
            );
        }

        #[test]
        fn simultaneous_startup_elects_one_primary_without_losing_other_batches() {
            let directory = tempfile::tempdir().unwrap();
            let barrier = Arc::new(std::sync::Barrier::new(8));
            let (sender, receiver) = std::sync::mpsc::channel();
            thread::scope(|scope| {
                for index in 0..8 {
                    let directory = directory.path();
                    let barrier = barrier.clone();
                    let sender = sender.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        let startup = start_in(
                            directory,
                            &[PathBuf::from(format!(r"C:\launch {index}.txt"))],
                        );
                        sender.send((index, startup)).unwrap();
                    });
                }
                let startups: Vec<_> = (0..8)
                    .map(|_| {
                        let (index, startup) =
                            receiver.recv_timeout(START_TIMEOUT + IO_TIMEOUT).unwrap();
                        (index, startup.unwrap())
                    })
                    .collect();
                let primaries: Vec<_> = startups
                    .iter()
                    .filter_map(|(index, startup)| match startup {
                        Startup::Primary(instance) => Some((*index, instance)),
                        Startup::Forwarded => None,
                    })
                    .collect();
                assert_eq!(
                    primaries.len(),
                    1,
                    "simultaneous launches must elect exactly one primary"
                );
                let (primary_index, primary) = primaries[0];
                let mut actual: Vec<_> = primary.take_requests().into_iter().flatten().collect();
                actual.sort();
                let expected: Vec<_> = (0..8)
                    .filter(|index| *index != primary_index)
                    .map(|index| PathBuf::from(format!(r"C:\launch {index}.txt")))
                    .collect();
                assert_eq!(
                    actual, expected,
                    "every non-primary launch must arrive as a complete request"
                );
            });
        }

        #[test]
        fn full_queue_retries_without_acknowledging_or_losing_new_requests() {
            let mut queue = Queue {
                accepting: true,
                batches: Vec::new(),
                bytes: 0,
                recent_ids: VecDeque::new(),
            };
            for index in 0..8 {
                assert_eq!(
                    queue.offer([index; 16], Vec::new(), MAX_PAYLOAD),
                    ACCEPTED,
                    "bounded queue must accept request {index}"
                );
            }
            assert_eq!(
                queue.offer([9; 16], Vec::new(), 1),
                RETRY,
                "full queue must not acknowledge a request it cannot store"
            );
            assert_eq!(
                queue.take().len(),
                8,
                "full queue must retain its accepted batches"
            );
            assert_eq!(
                queue.offer([9; 16], Vec::new(), 1),
                ACCEPTED,
                "draining the queue must let the declined request retry"
            );
        }

        #[test]
        fn shutdown_declines_requests_until_resumed_or_replaced() {
            let directory = tempfile::tempdir().unwrap();
            let primary = primary(directory.path());
            assert!(
                primary.stop_accepting().is_empty(),
                "new primary has no pending requests"
            );
            thread::scope(|scope| {
                let (sender, receiver) = std::sync::mpsc::channel();
                let directory = directory.path();
                let client = scope.spawn(move || {
                    sender
                        .send(start_in(
                            directory,
                            &[PathBuf::from(r"C:\during shutdown.txt")],
                        ))
                        .unwrap();
                });
                assert!(
                    receiver.recv_timeout(Duration::from_millis(100)).is_err(),
                    "stopping primary must not acknowledge new paths"
                );
                primary.resume_accepting();
                assert!(
                    matches!(
                        receiver.recv_timeout(START_TIMEOUT).unwrap().unwrap(),
                        Startup::Forwarded
                    ),
                    "deferred shutdown must resume forwarding"
                );
                client.join().unwrap();
            });
            assert_eq!(
                primary.stop_accepting(),
                vec![vec![PathBuf::from(r"C:\during shutdown.txt")]],
                "shutdown must return all already-accepted paths"
            );
            drop(primary);
            let successor = match start_in(directory.path(), &[]).unwrap() {
                Startup::Primary(instance) => instance,
                Startup::Forwarded => {
                    panic!("stopped primary must release election to a successor")
                }
            };
            assert!(
                successor.take_requests().is_empty(),
                "successor starts without phantom requests"
            );
        }

        #[test]
        fn data_directories_are_isolated_and_stale_lock_files_do_not_block() {
            let first = tempfile::tempdir().unwrap();
            let second = tempfile::tempdir().unwrap();
            fs::write(first.path().join("instance.lock"), b"stale").unwrap();
            let first_primary = primary(first.path());
            let second_primary = primary(second.path());
            assert!(
                matches!(
                    start_in(second.path(), &[PathBuf::from(r"C:\second.txt")]).unwrap(),
                    Startup::Forwarded
                ),
                "second data directory must have its own primary"
            );
            assert!(
                first_primary.take_requests().is_empty(),
                "isolated directory must not receive another directory's paths"
            );
            assert_eq!(
                second_primary.take_requests(),
                vec![vec![PathBuf::from(r"C:\second.txt")]],
                "isolated primary must receive its own paths"
            );
        }

        #[test]
        fn retries_are_idempotent_and_malformed_payloads_never_queue() {
            let directory = tempfile::tempdir().unwrap();
            let primary = primary(directory.path());
            let lock = File::open(directory.path().join("instance.lock")).unwrap();
            // SAFETY: GetCurrentProcess returns a valid borrowed pseudo-handle.
            let sid = process_sid(unsafe { GetCurrentProcess() }).unwrap();
            let name = pipe_name(&lock, &sid).unwrap();
            let payload = encode(&[PathBuf::from(r"C:\once.txt")], [42; 16]).unwrap();
            // Reconnect readiness is distinct from a protocol response, as in start_in's retry loop.
            let request = |payload: &[u8]| {
                let deadline = Instant::now() + START_TIMEOUT;
                loop {
                    let response = forward(&name, &sid, payload).expect("handoff response");
                    if response != RETRY {
                        break response;
                    }
                    assert!(Instant::now() < deadline, "handoff did not become ready");
                    thread::sleep(RETRY_DELAY);
                }
            };
            assert_eq!(request(&payload), ACCEPTED, "first request must queue");
            assert_eq!(
                request(&payload),
                ACCEPTED,
                "retry must acknowledge existing request"
            );
            assert_eq!(
                request(b"bad payload"),
                INVALID,
                "malformed request must be rejected"
            );
            assert_eq!(
                primary.take_requests(),
                vec![vec![PathBuf::from(r"C:\once.txt")]],
                "retry must not duplicate a queued batch"
            );
        }
    }
}
