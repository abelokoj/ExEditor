//! Thread-confined binding to ExEditor's packaged VFST-only libvoikko runtime.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    ptr::NonNull,
};

use anyhow::{Context, Result, bail, ensure};
use libloading::Library;

use super::MAX_SUGGESTIONS;

type Init = unsafe extern "C" fn(*mut *const c_char, *const c_char, *const c_char) -> *mut c_void;
type Terminate = unsafe extern "C" fn(*mut c_void);
type Spell = unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int;
type Suggest = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut *mut c_char;
type FreeSuggestions = unsafe extern "C" fn(*mut *mut c_char);

pub(super) struct Voikko {
    handle: NonNull<c_void>,
    terminate: Terminate,
    spell: Spell,
    suggest: Suggest,
    free_suggestions: FreeSuggestions,
    // The library must outlive the native handle and all copied function pointers.
    _library: Library,
}

impl Voikko {
    pub(super) fn open(language: &str, dictionary_root: &Path) -> Result<Self> {
        let executable =
            std::env::current_exe().context("Locating ExEditor's native spelling runtime")?;
        let runtime = executable
            .parent()
            .context("ExEditor has no application folder")?
            .join("proofing")
            .join("libvoikko-1.dll");
        ensure!(
            runtime.is_file(),
            "The packaged Voikko runtime is missing. Reinstall the complete ExEditor package; the proofing folder must remain beside ExEditor.exe."
        );
        let library = load_runtime(&runtime)?;
        // SAFETY: these are the stable C ABI signatures declared in libvoikko 4.3.3's voikko.h.
        // Every pointer is copied while the library is alive and retained in this owner.
        let (initialize, terminate, spell, suggest, free_suggestions) = unsafe {
            (
                *library
                    .get::<Init>(b"voikkoInit\0")
                    .context("The Voikko runtime does not export voikkoInit")?,
                *library
                    .get::<Terminate>(b"voikkoTerminate\0")
                    .context("The Voikko runtime does not export voikkoTerminate")?,
                *library
                    .get::<Spell>(b"voikkoSpellCstr\0")
                    .context("The Voikko runtime does not export voikkoSpellCstr")?,
                *library
                    .get::<Suggest>(b"voikkoSuggestCstr\0")
                    .context("The Voikko runtime does not export voikkoSuggestCstr")?,
                *library
                    .get::<FreeSuggestions>(b"voikkoFreeCstrArray\0")
                    .context("The Voikko runtime does not export voikkoFreeCstrArray")?,
            )
        };
        let language =
            CString::new(language).context("The Voikko language contains a NUL character")?;
        let path = CString::new(
            dictionary_root
                .to_str()
                .context("The dictionary folder is not valid Unicode")?,
        )
        .context("The dictionary folder contains a NUL character")?;
        let mut error = std::ptr::null();
        // SAFETY: both arguments are NUL-terminated UTF-8 strings valid for this call;
        // error is a live writable out parameter, and a successful handle is uniquely owned.
        let handle = unsafe { initialize(&mut error, language.as_ptr(), path.as_ptr()) };
        let Some(handle) = NonNull::new(handle) else {
            let message = if error.is_null() {
                "No compatible dictionary matches the selected language.".into()
            } else {
                // SAFETY: voikkoInit returns a borrowed, NUL-terminated diagnostic on failure.
                unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned()
            };
            bail!(
                "Voikko could not load the selected dictionary: {message} Choose its root folder containing 5/mor-*/index.txt and mor.vfst, and use its language code (usually fi)."
            );
        };
        Ok(Self {
            handle,
            terminate,
            spell,
            suggest,
            free_suggestions,
            _library: library,
        })
    }

    pub(super) fn word(&self, word: &str) -> Result<Option<Vec<String>>> {
        let input = CString::new(word).context("The word contains a NUL character")?;
        // SAFETY: the handle and DLL remain alive, the UTF-8 word is NUL-terminated,
        // and this object is accessed only on its owning worker thread.
        match unsafe { (self.spell)(self.handle.as_ptr(), input.as_ptr()) } {
            1 => return Ok(None),
            0 => {}
            2 => bail!("Voikko reported an internal dictionary error while checking “{word}”."),
            3 => bail!("Voikko could not decode the word “{word}”."),
            status => bail!("Voikko returned an unknown spelling status ({status})."),
        }
        // SAFETY: a misspelling may return a null pointer (no suggestions), or a
        // library-owned, NUL-terminated array which must be freed by the matching DLL.
        let pointer = unsafe { (self.suggest)(self.handle.as_ptr(), input.as_ptr()) };
        if pointer.is_null() {
            return Ok(Some(Vec::new()));
        }
        let array = SuggestionArray {
            pointer,
            free: self.free_suggestions,
        };
        let mut suggestions = Vec::new();
        for index in 0..MAX_SUGGESTIONS {
            // SAFETY: the C API guarantees a NUL-terminated pointer array; the loop
            // stops at the first terminator and its RAII owner remains alive.
            let pointer = unsafe { *array.pointer.add(index) };
            if pointer.is_null() {
                break;
            }
            // SAFETY: each non-null entry is a NUL-terminated UTF-8 string owned by array.
            let suggestion = unsafe { CStr::from_ptr(pointer) }
                .to_str()
                .context("Voikko returned an invalid UTF-8 suggestion")?;
            if suggestion != word && !suggestions.iter().any(|existing| existing == suggestion) {
                suggestions.push(suggestion.to_owned());
            }
        }
        Ok(Some(suggestions))
    }
}

impl Drop for Voikko {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of the initialized handle, and the DLL's
        // Library field is dropped only after this destructor returns.
        unsafe { (self.terminate)(self.handle.as_ptr()) };
    }
}
struct SuggestionArray {
    pointer: *mut *mut c_char,
    free: FreeSuggestions,
}
impl Drop for SuggestionArray {
    fn drop(&mut self) {
        // SAFETY: this array came from voikkoSuggestCstr and is freed exactly once
        // using the same DLL's matching function while that DLL is still loaded.
        unsafe { (self.free)(self.pointer) };
    }
}

#[cfg(windows)]
fn load_runtime(path: &Path) -> Result<Library> {
    // Normalize separators before the Windows loader sees an extended-length path.
    let path = std::fs::canonicalize(path)
        .with_context(|| format!("Locating the packaged Voikko runtime at {}", path.display()))?;
    // SAFETY: the absolute library path is supplied by this app, never by a document.
    // DLL_LOAD_DIR | DEFAULT_DIRS restrict dependency loading to the packaged DLL's
    // directory and normal system/application locations, excluding the working directory.
    let library = unsafe {
        libloading::os::windows::Library::load_with_flags(&path, 0x0000_0100 | 0x0000_1000)
    }
    .context("Loading the packaged Voikko runtime and its dependencies")?;
    Ok(library.into())
}
#[cfg(not(windows))]
fn load_runtime(_path: &Path) -> Result<Library> {
    bail!("This ExEditor package currently supplies the Windows Voikko runtime only.")
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn packaged_native_runtime_loads_real_abi_exports_and_version() {
        // GitHub stages the actual produced runtime beside the test executable,
        // preserving the production search restriction rather than altering PATH.
        let executable = std::env::current_exe().expect("test executable");
        let runtime = executable
            .parent()
            .expect("test folder")
            .join("proofing")
            .join("libvoikko-1.dll");
        let library = load_runtime(&runtime).expect("GitHub-built packaged Voikko runtime");
        type GetVersion = unsafe extern "C" fn() -> *const c_char;
        // SAFETY: the signatures are the stable libvoikko 4.3.3 C ABI. The DLL
        // remains alive while symbols and its borrowed version string are used.
        let version = unsafe {
            library.get::<Init>(b"voikkoInit\0").expect("voikkoInit");
            library
                .get::<Terminate>(b"voikkoTerminate\0")
                .expect("voikkoTerminate");
            library
                .get::<Spell>(b"voikkoSpellCstr\0")
                .expect("voikkoSpellCstr");
            library
                .get::<Suggest>(b"voikkoSuggestCstr\0")
                .expect("voikkoSuggestCstr");
            library
                .get::<FreeSuggestions>(b"voikkoFreeCstrArray\0")
                .expect("voikkoFreeCstrArray");
            let get_version = library
                .get::<GetVersion>(b"voikkoGetVersion\0")
                .expect("voikkoGetVersion");
            let pointer = get_version();
            assert!(!pointer.is_null(), "native version string");
            CStr::from_ptr(pointer)
                .to_str()
                .expect("UTF-8 native version")
                .to_owned()
        };
        assert_eq!(version, "4.3.3");
    }
}
