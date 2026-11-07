//! Per-user Unix socket handoff; a private directory and a held file lock select the owner.
use anyhow::{Context as _, Result, bail, ensure};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read as _, Write as _},
    os::unix::{
        ffi::{OsStrExt as _, OsStringExt as _},
        fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_PAYLOAD: usize = 1024 * 1024;
const MAX_BATCHES: usize = 128;
const MAX_QUEUED_BYTES: usize = 8 * MAX_PAYLOAD;
struct State {
    accepting: AtomicBool,
    running: AtomicBool,
    queue: Mutex<VecDeque<Vec<PathBuf>>>,
}
pub(crate) enum Startup {
    Primary(Instance),
    Forwarded,
}
pub(crate) struct Instance {
    state: Arc<State>,
    thread: Option<thread::JoinHandle<()>>,
    socket: PathBuf,
    _lock: File,
}

pub(crate) fn start(paths: &[PathBuf]) -> Result<Startup> {
    let data = crate::document::data_dir()?.canonicalize()?;
    // SAFETY: geteuid only reads this process's effective user identifier.
    let uid = unsafe { libc::geteuid() };
    let hash = data
        .as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    let directory = std::env::temp_dir().join(format!("exeditor-{uid}-{hash:016x}"));
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error).context("creating the private editor handoff directory"),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    ensure!(
        metadata.is_dir() && metadata.uid() == uid && (metadata.permissions().mode() & 0o077) == 0,
        "The editor handoff directory is not private to this user."
    );
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(directory.join("owner.lock"))?;
    let socket = directory.join("open.sock");
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            forward(&socket, paths)?;
            return Ok(Startup::Forwarded);
        }
        Err(TryLockError::Error(error)) => return Err(error).context("locking the editor handoff"),
    }
    match fs::remove_file(&socket) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("removing the inactive editor socket"),
    }
    let listener = UnixListener::bind(&socket).context("opening the editor handoff socket")?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let state = Arc::new(State {
        accepting: AtomicBool::new(true),
        running: AtomicBool::new(true),
        queue: Mutex::new(VecDeque::new()),
    });
    let receiver = state.clone();
    let worker = thread::Builder::new()
        .name("exeditor-file-open".into())
        .spawn(move || {
            while receiver.running.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
                        let _ = receive(&mut stream, &receiver);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(25))
                    }
                    Err(_) => break,
                }
            }
        })?;
    Ok(Startup::Primary(Instance {
        state,
        thread: Some(worker),
        socket,
        _lock: lock,
    }))
}
fn encode(paths: &[PathBuf]) -> Result<Vec<u8>> {
    ensure!(
        paths.len() <= 1024,
        "Too many files were requested at once."
    );
    let paths = paths
        .iter()
        .map(|path| {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()?.join(path)
            };
            Ok::<_, std::io::Error>(path.into_os_string().into_vec())
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let bytes = serde_json::to_vec(&paths)?;
    ensure!(
        bytes.len() <= MAX_PAYLOAD,
        "The file-open request is too large."
    );
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Vec<PathBuf>> {
    ensure!(
        bytes.len() <= MAX_PAYLOAD,
        "The file-open request is too large."
    );
    let paths: Vec<Vec<u8>> = serde_json::from_slice(bytes)?;
    ensure!(
        paths.len() <= 1024,
        "Too many files were requested at once."
    );
    paths
        .into_iter()
        .map(|path| {
            ensure!(
                !path.is_empty() && !path.contains(&0),
                "The requested path is invalid."
            );
            let path = PathBuf::from(std::ffi::OsString::from_vec(path));
            ensure!(path.is_absolute(), "The requested path must be absolute.");
            Ok(path)
        })
        .collect()
}
fn receive(stream: &mut UnixStream, state: &State) -> Result<()> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    ensure!(length <= MAX_PAYLOAD, "The file-open request is too large.");
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    let paths = decode(&bytes)?;
    let mut queue = state
        .queue
        .lock()
        .map_err(|_| anyhow::anyhow!("The editor request queue is unavailable."))?;
    let bytes: usize = queue
        .iter()
        .flatten()
        .map(|path| path.as_os_str().as_bytes().len())
        .sum();
    let incoming: usize = paths
        .iter()
        .map(|path| path.as_os_str().as_bytes().len())
        .sum();
    if !state.accepting.load(Ordering::Acquire)
        || queue.len() >= MAX_BATCHES
        || bytes.saturating_add(incoming) > MAX_QUEUED_BYTES
    {
        stream.write_all(&[0])?;
    } else {
        queue.push_back(paths);
        stream.write_all(&[1])?;
    }
    Ok(())
}
fn forward(socket: &std::path::Path, paths: &[PathBuf]) -> Result<()> {
    let payload = encode(paths)?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(mut stream) = UnixStream::connect(socket) {
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            stream.write_all(&(payload.len() as u32).to_be_bytes())?;
            stream.write_all(&payload)?;
            let mut accepted = [0];
            if stream.read_exact(&mut accepted).is_ok() && accepted == [1] {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            bail!(
                "The running ExEditor could not accept these files. Keep it open and retry; no files have been changed."
            );
        }
        thread::sleep(Duration::from_millis(40));
    }
}
impl Instance {
    pub(crate) fn take_requests(&self) -> Vec<Vec<PathBuf>> {
        self.state
            .queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default()
    }
    pub(crate) fn stop_accepting(&self) -> Vec<Vec<PathBuf>> {
        self.state.accepting.store(false, Ordering::Release);
        self.take_requests()
    }
    pub(crate) fn resume_accepting(&self) {
        self.state.accepting.store(true, Ordering::Release);
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.state.running.store(false, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handoff_preserves_non_utf8_paths_and_rejects_relative_or_nul_payloads() {
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/note-\xff.md".to_vec()));
        assert_eq!(
            decode(&encode(std::slice::from_ref(&path)).unwrap()).unwrap(),
            [path]
        );
        assert!(decode(br"[[110,111,116,101]]").is_err());
        assert!(decode(br"[[47,0]]").is_err());
    }
    #[test]
    fn paused_receiver_keeps_requests_with_the_client() {
        let state = State {
            accepting: AtomicBool::new(false),
            running: AtomicBool::new(true),
            queue: Mutex::new(VecDeque::new()),
        };
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let payload = encode(&[PathBuf::from("/tmp/a.md")]).unwrap();
        client
            .write_all(&(payload.len() as u32).to_be_bytes())
            .unwrap();
        client.write_all(&payload).unwrap();
        receive(&mut server, &state).unwrap();
        let mut ack = [9];
        client.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [0]);
        assert!(state.queue.lock().unwrap().is_empty());
        state.accepting.store(true, Ordering::Release);
        client
            .write_all(&(payload.len() as u32).to_be_bytes())
            .unwrap();
        client.write_all(&payload).unwrap();
        receive(&mut server, &state).unwrap();
        client.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [1]);
        assert_eq!(state.queue.lock().unwrap().len(), 1);
    }
}
