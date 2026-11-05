//! Local document formats, safe saves, version snapshots, and crash recovery.

use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use directories::ProjectDirs;
use encoding_rs::WINDOWS_1252;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

const MAX_DOCUMENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 10_000;
const VERSION_LIMIT: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DocumentFormat {
    PlainText,
    #[default]
    Rtf,
    Rtfd,
    Html,
    Docx,
    Odt,
    Doc,
}

impl DocumentFormat {
    pub fn from_path(path: &Path) -> Result<Self> {
        let extension = path.extension().and_then(OsStr::to_str).unwrap_or("");
        Ok(match extension.to_ascii_lowercase().as_str() {
            "rtf" => Self::Rtf,
            "rtfd" => Self::Rtfd,
            "html" | "htm" => Self::Html,
            "docx" => Self::Docx,
            "odt" => Self::Odt,
            "doc" => Self::Doc,
            _ if path.is_dir() => bail!("This folder is not an RTFD document"),
            _ => Self::PlainText,
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::PlainText => "txt",
            Self::Rtf => "rtf",
            Self::Rtfd => "rtfd",
            Self::Html => "html",
            Self::Docx => "docx",
            Self::Odt => "odt",
            Self::Doc => "doc",
        }
    }

    pub fn is_plain(self) -> bool {
        self == Self::PlainText
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentContent {
    Plain(String),
    Rich(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "Unicode (UTF-8)",
            Self::Utf8Bom => "Unicode (UTF-8 with BOM)",
            Self::Utf16Le => "Unicode (UTF-16 little endian)",
            Self::Utf16Be => "Unicode (UTF-16 big endian)",
            Self::Windows1252 => "Western (Windows Latin 1)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFingerprint {
    length: u64,
    modified_nanos: u128,
    digest: u64,
}

#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub content: DocumentContent,
    pub path: PathBuf,
    pub format: DocumentFormat,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub fingerprint: FileFingerprint,
}

pub fn load(path: &Path) -> Result<LoadedDocument> {
    load_using_encoding(path, None)
}

/// Opens a legacy text file using an explicitly selected encoding.
pub fn load_with_encoding(path: &Path, encoding: TextEncoding) -> Result<LoadedDocument> {
    load_using_encoding(path, Some(encoding))
}

fn load_using_encoding(
    path: &Path,
    selected_encoding: Option<TextEncoding>,
) -> Result<LoadedDocument> {
    let path = fs::canonicalize(path).with_context(|| format!("Opening {}", path.display()))?;
    let format = DocumentFormat::from_path(&path)?;
    let before = fingerprint(&path)?;
    let (content, encoding, line_ending) = match format {
        DocumentFormat::PlainText => {
            let bytes = read_document(&path)?;
            let (text, encoding) = decode_text(&bytes, selected_encoding)?;
            let line_ending = detect_line_ending(&text);
            (
                DocumentContent::Plain(normalize_lines(&text)),
                encoding,
                line_ending,
            )
        }
        DocumentFormat::Rtf => {
            let bytes = read_document(&path)?;
            validate_rtf(&bytes)?;
            ensure!(
                !has_external_graphics(&bytes),
                "This RTF refers to external attachments. Open its RTFD folder instead so attachments are preserved."
            );
            (
                DocumentContent::Rich(bytes),
                TextEncoding::Utf8,
                LineEnding::Lf,
            )
        }
        DocumentFormat::Rtfd => (
            DocumentContent::Rich(read_rtfd(&path)?),
            TextEncoding::Utf8,
            LineEnding::Lf,
        ),
        _ => {
            let converted = crate::formats::import(&path, format)?;
            validate_rtf(&converted)?;
            (
                DocumentContent::Rich(converted),
                TextEncoding::Utf8,
                LineEnding::Lf,
            )
        }
    };
    ensure!(
        before == fingerprint(&path)?,
        "The document changed while it was being opened. Open it again to read the current version."
    );
    Ok(LoadedDocument {
        content,
        path,
        format,
        encoding,
        line_ending,
        fingerprint: before,
    })
}

pub fn save(
    path: &Path,
    format: DocumentFormat,
    content: &DocumentContent,
    encoding: TextEncoding,
    line_ending: LineEnding,
) -> Result<FileFingerprint> {
    let existed = path.try_exists()?;
    let before = existed.then(|| fingerprint(path)).transpose()?;
    if existed {
        ensure!(
            !fs::metadata(path)?.permissions().readonly(),
            "This document is read-only. Use Save As to save a writable copy."
        );
        ensure!(
            path.is_dir() == (format == DocumentFormat::Rtfd),
            "The destination is a different kind of file. Choose another name."
        );
    }
    let output = match format {
        DocumentFormat::PlainText => {
            let DocumentContent::Plain(text) = content else {
                bail!(
                    "Convert the document to plain text before saving it as a text file; formatting would otherwise be lost."
                );
            };
            encode_text(text, encoding, line_ending)?
        }
        DocumentFormat::Rtf | DocumentFormat::Rtfd => content_as_rtf(content)?,
        _ => crate::formats::export(&content_as_rtf(content)?, format)?,
    };
    let now = path.try_exists()?.then(|| fingerprint(path)).transpose()?;
    ensure!(
        now == before,
        "The original changed while saving. Your edits are still open; use Save As or reopen the changed file."
    );
    if existed {
        snapshot(path)?;
    }
    if format == DocumentFormat::Rtfd {
        if !existed {
            fs::create_dir(path).with_context(|| format!("Creating {}", path.display()))?;
        }
        // Existing attachment files remain intact when the embedded RTF is replaced.
        atomic_write(&path.join("TXT.rtf"), &output)?;
    } else {
        atomic_write(path, &output)?;
    }
    fingerprint(path)
}

fn read_document(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("Reading {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_DOCUMENT_BYTES,
        "The document exceeds the 512 MiB safety limit."
    );
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_DOCUMENT_BYTES,
        "The document exceeds the 512 MiB safety limit."
    );
    Ok(bytes)
}

fn validate_rtf(bytes: &[u8]) -> Result<()> {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(0);
    ensure!(
        bytes.get(start..start + 5) == Some(b"{\\rtf"),
        "This file does not contain a valid RTF document."
    );
    Ok(())
}

fn decode_text(bytes: &[u8], selected: Option<TextEncoding>) -> Result<(String, TextEncoding)> {
    let (encoding, bytes) = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        (TextEncoding::Utf8Bom, &bytes[3..])
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        (TextEncoding::Utf16Le, &bytes[2..])
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (TextEncoding::Utf16Be, &bytes[2..])
    } else {
        (selected.unwrap_or_default(), bytes)
    };
    let text = match encoding {
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => String::from_utf8(bytes.to_vec()).context("This file is not UTF-8. Reopen it with its original text encoding; no bytes were replaced.")?,
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            ensure!(bytes.len().is_multiple_of(2), "The UTF-16 file ends in an incomplete code unit.");
            let words: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| {
                if encoding == TextEncoding::Utf16Le { u16::from_le_bytes([pair[0], pair[1]]) } else { u16::from_be_bytes([pair[0], pair[1]]) }
            }).collect();
            String::from_utf16(&words).context("The UTF-16 file contains an invalid surrogate sequence.")?
        }
        TextEncoding::Windows1252 => {
            let (text, had_errors) = WINDOWS_1252.decode_without_bom_handling(bytes);
            ensure!(!had_errors, "This file contains undefined Windows Latin 1 byte sequences; no replacement characters were inserted.");
            text.into_owned()
        }
    };
    ensure!(
        !text.contains('\0'),
        "This file contains NUL bytes and may be binary. Choose its correct text encoding."
    );
    Ok((text, encoding))
}

fn normalize_lines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn detect_line_ending(text: &str) -> LineEnding {
    if text.contains("\r\n") {
        LineEnding::CrLf
    } else if text.contains('\r') {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

fn encode_text(text: &str, encoding: TextEncoding, ending: LineEnding) -> Result<Vec<u8>> {
    let normalized = normalize_lines(text);
    let text = match ending {
        LineEnding::Lf => normalized,
        LineEnding::CrLf => normalized.replace('\n', "\r\n"),
        LineEnding::Cr => normalized.replace('\n', "\r"),
    };
    Ok(match encoding {
        TextEncoding::Utf8 => text.into_bytes(),
        TextEncoding::Utf8Bom => [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat(),
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            let mut output = if encoding == TextEncoding::Utf16Le {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for word in text.encode_utf16() {
                output.extend_from_slice(&if encoding == TextEncoding::Utf16Le {
                    word.to_le_bytes()
                } else {
                    word.to_be_bytes()
                });
            }
            output
        }
        TextEncoding::Windows1252 => {
            let (output, _, had_errors) = WINDOWS_1252.encode(&text);
            ensure!(
                !had_errors,
                "This document contains characters that Windows Latin 1 cannot represent. Choose a Unicode encoding instead."
            );
            output.into_owned()
        }
    })
}

fn content_as_rtf(content: &DocumentContent) -> Result<Vec<u8>> {
    if let DocumentContent::Rich(bytes) = content {
        validate_rtf(bytes)?;
        ensure!(
            !has_external_graphics(bytes),
            "External RTF attachments must be embedded before saving."
        );
        return Ok(bytes.clone());
    }
    let DocumentContent::Plain(text) = content else {
        unreachable!()
    };
    let mut rtf = String::from("{\\rtf1\\ansi\\ansicpg1252\\uc1 ");
    for word in normalize_lines(text).encode_utf16() {
        match word {
            10 => rtf.push_str("\\par\n"),
            9 => rtf.push_str("\\tab "),
            92 | 123 | 125 => {
                rtf.push('\\');
                rtf.push(char::from_u32(u32::from(word)).expect("ASCII code unit"));
            }
            32..=126 => rtf.push(char::from_u32(u32::from(word)).expect("ASCII code unit")),
            _ => {
                use std::fmt::Write as _;
                write!(rtf, "\\u{}?", word as i16).expect("writing into a String cannot fail");
            }
        }
    }
    rtf.push('}');
    Ok(rtf.into_bytes())
}

pub(crate) fn data_dir() -> Result<PathBuf> {
    // EXEDITOR_DATA_DIR isolates test runs from the user's preferences and recovery buffers.
    let path = match std::env::var_os("EXEDITOR_DATA_DIR") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => ProjectDirs::from("com", "ExEditor", "ExEditor")
            .context("The operating system did not provide a local application-data folder")?
            .data_local_dir()
            .to_path_buf(),
    };
    fs::create_dir_all(&path).with_context(|| format!("Creating {}", path.display()))?;
    Ok(path)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let permissions = path
        .try_exists()?
        .then(|| fs::metadata(path).map(|metadata| metadata.permissions()))
        .transpose()?;
    let mut temporary = NamedTempFile::new_in(parent)
        .context("Creating a safe replacement file beside the original")?;
    temporary
        .write_all(bytes)
        .context("Writing the replacement document")?;
    temporary
        .as_file_mut()
        .sync_all()
        .context("Flushing the replacement document")?;
    if let Some(permissions) = permissions {
        ensure!(
            !permissions.readonly(),
            "The destination is read-only; the original was preserved."
        );
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "Replacing {} safely; the original is preserved if replacement fails",
                path.display()
            )
        })?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
}

pub fn fingerprint(path: &Path) -> Result<FileFingerprint> {
    let mut files = Vec::new();
    if path.is_dir() {
        collect_bundle_files(path, &mut files)?;
    } else {
        files.push(path.to_path_buf());
    }
    files.sort();
    let mut result = FileFingerprint {
        length: 0,
        modified_nanos: 0,
        digest: 0xcbf29ce484222325,
    };
    for file in files {
        let metadata = fs::metadata(&file)?;
        ensure!(
            metadata.len() <= MAX_DOCUMENT_BYTES,
            "An attachment exceeds the 512 MiB safety limit."
        );
        result.length = result
            .length
            .checked_add(metadata.len())
            .context("The document is too large")?;
        ensure!(
            result.length <= MAX_DOCUMENT_BYTES,
            "The document and attachments exceed the 512 MiB safety limit."
        );
        result.modified_nanos = result.modified_nanos.max(
            metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        if path.is_dir() {
            hash_bytes(
                &mut result.digest,
                file.strip_prefix(path)?.to_string_lossy().as_bytes(),
            );
        }
        let mut input = File::open(&file)?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash_bytes(&mut result.digest, &buffer[..count]);
        }
    }
    Ok(result)
}

fn collect_bundle_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "RTFD bundles with symbolic links cannot be opened safely."
        );
        if kind.is_dir() {
            collect_bundle_files(&entry.path(), files)?;
        } else if kind.is_file() {
            files.push(entry.path());
            ensure!(
                files.len() <= MAX_BUNDLE_FILES,
                "The RTFD bundle contains too many attachments."
            );
        }
    }
    Ok(())
}

fn history_dir(path: &Path) -> Result<PathBuf> {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hash = 0xcbf29ce484222325;
    #[cfg(windows)]
    let identity = absolute.to_string_lossy().to_lowercase();
    #[cfg(not(windows))]
    let identity = absolute.to_string_lossy().into_owned();
    hash_bytes(&mut hash, identity.as_bytes());
    Ok(data_dir()?.join("versions").join(format!("{hash:016x}")))
}

fn snapshot(path: &Path) -> Result<()> {
    let directory = history_dir(path)?;
    fs::create_dir_all(&directory)?;
    let extension = if path.is_dir() {
        "rtf"
    } else {
        path.extension().and_then(OsStr::to_str).unwrap_or("txt")
    };
    let name = format!("{}.{}", Utc::now().format("%Y%m%dT%H%M%S%.9fZ"), extension);
    let bytes = if path.is_dir() {
        read_rtfd(path)?
    } else {
        read_document(path)?
    };
    atomic_write(&directory.join(name), &bytes)?;
    let mut versions = list_versions(path)?;
    versions.sort();
    let remove_count = versions.len().saturating_sub(VERSION_LIMIT);
    for version in versions.into_iter().take(remove_count) {
        ensure!(
            version.parent() == Some(directory.as_path()),
            "Invalid document version path"
        );
        fs::remove_file(version)?;
    }
    Ok(())
}

pub fn list_versions(path: &Path) -> Result<Vec<PathBuf>> {
    let directory = history_dir(path)?;
    if !directory.try_exists()? {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            paths.push(entry.path());
        }
    }
    paths.sort_by(|left, right| right.cmp(left));
    Ok(paths)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryDocument {
    pub id: String,
    pub original_path: Option<PathBuf>,
    pub format: DocumentFormat,
    pub content: DocumentContent,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub saved_at: String,
}

pub(crate) fn validate_recovery_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "Invalid recovery-document identifier"
    );
    Ok(())
}

pub(crate) fn recovery_dir() -> Result<PathBuf> {
    let folder = data_dir()?.join("recovery");
    fs::create_dir_all(&folder).context("Creating the unsaved-buffer folder")?;
    Ok(folder)
}

fn recovery_path(id: &str) -> Result<PathBuf> {
    validate_recovery_id(id)?;
    Ok(recovery_dir()?.join(format!("{id}.json")))
}

struct LimitedWriter<W> {
    inner: W,
    remaining: u64,
}

impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other(
                "The unsaved buffer exceeds the 512 MiB recovery limit",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub fn write_recovery(document: &RecoveryDocument) -> Result<()> {
    let path = recovery_path(&document.id)?;
    let parent = path
        .parent()
        .context("The recovery snapshot has no parent folder")?;
    let permissions = path
        .try_exists()?
        .then(|| fs::metadata(&path).map(|metadata| metadata.permissions()))
        .transpose()?;
    if let Some(permissions) = &permissions {
        ensure!(
            !permissions.readonly(),
            "The unsaved-buffer snapshot is read-only; its previous contents were preserved."
        );
    }
    let mut temporary = NamedTempFile::new_in(parent)
        .context("Creating a safe replacement for the unsaved-buffer snapshot")?;
    {
        // Stream into a bounded temporary file rather than allocating a second document-sized JSON buffer.
        let mut writer = std::io::BufWriter::new(LimitedWriter {
            inner: temporary.as_file_mut(),
            remaining: MAX_DOCUMENT_BYTES,
        });
        serde_json::to_writer(&mut writer, document)
            .context("Writing the unsaved buffer; its previous snapshot was preserved")?;
        writer
            .flush()
            .context("Flushing the unsaved-buffer snapshot")?;
    }
    temporary
        .as_file_mut()
        .sync_all()
        .context("Saving the unsaved-buffer snapshot to disk")?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.persist(&path).map_err(|error| error.error)
        .context("Replacing the unsaved-buffer snapshot; its previous contents were preserved if replacement failed")?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// Reads one bounded buffer snapshot without changing or removing its bytes.
pub fn read_recovery(path: &Path) -> Result<RecoveryDocument> {
    ensure!(
        path.extension() == Some(OsStr::new("json")),
        "The unsaved buffer does not have a recovery snapshot file name."
    );
    let id = path
        .file_stem()
        .and_then(OsStr::to_str)
        .context("The unsaved buffer has an invalid file name")?;
    validate_recovery_id(id)?;
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "The unsaved buffer is not a regular file; it has been preserved."
    );
    let document: RecoveryDocument =
        serde_json::from_slice(&read_document(path)?).with_context(|| {
            format!(
                "Reading unsaved buffer {}; it has been preserved for manual recovery",
                path.display()
            )
        })?;
    validate_recovery_id(&document.id)?;
    ensure!(
        document.id == id,
        "The recovery file identifier does not match its file name."
    );
    match &document.content {
        DocumentContent::Plain(text) => ensure!(
            !text.contains('\0'),
            "The unsaved text buffer contains NUL characters; its snapshot has been preserved."
        ),
        DocumentContent::Rich(bytes) => validate_rtf(bytes)
            .context("The unsaved rich-text buffer is invalid; its snapshot has been preserved")?,
    }
    chrono::DateTime::parse_from_rfc3339(&document.saved_at)
        .context("The unsaved buffer has an invalid timestamp; its snapshot has been preserved")?;
    Ok(document)
}

pub fn remove_recovery(id: &str) -> Result<()> {
    let path = recovery_path(id)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn has_external_graphics(bytes: &[u8]) -> bool {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'\\' {
            cursor += 1;
            continue;
        }
        cursor += 1;
        let Some(first) = bytes.get(cursor) else {
            break;
        };
        if !first.is_ascii_alphabetic() {
            cursor = cursor.saturating_add(if *first == b'\'' { 3 } else { 1 });
            continue;
        }
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
            cursor += 1;
        }
        let word = &bytes[start..cursor];
        if word == b"NeXTGraphic" {
            return true;
        }
        let number_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if word == b"bin" {
            let count = std::str::from_utf8(&bytes[number_start..cursor])
                .ok()
                .and_then(|text| text.parse::<usize>().ok());
            if bytes.get(cursor) == Some(&b' ') {
                cursor += 1;
            }
            let Some(end) = count.and_then(|count| cursor.checked_add(count)) else {
                return true;
            };
            if end > bytes.len() {
                return true;
            }
            cursor = end;
        }
    }
    false
}

fn read_rtfd(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        path.is_dir(),
        "An RTFD document must be a folder containing TXT.rtf."
    );
    let mut files = Vec::new();
    collect_bundle_files(path, &mut files)?;
    let bytes = read_document(&path.join("TXT.rtf"))?;
    validate_rtf(&bytes)?;
    if !has_external_graphics(&bytes) {
        return Ok(bytes);
    }
    let spans = graphic_groups(&bytes)?;
    ensure!(
        !spans.is_empty(),
        "The RTFD attachment syntax is not supported; the original bundle was preserved."
    );
    let mut output = Vec::new();
    let mut copied = 0;
    for (start, end, name_start, name_end) in spans {
        output.extend_from_slice(&bytes[copied..start]);
        let name = std::str::from_utf8(&bytes[name_start..name_end])
            .context("An RTFD attachment name is not UTF-8")?
            .trim();
        ensure!(
            !name.is_empty()
                && Path::new(name)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_)))
                && !name.contains(['/', '\\']),
            "The RTFD attachment name must name a file inside its bundle."
        );
        let attachment = path.join(name);
        let root = fs::canonicalize(path)?;
        let resolved = fs::canonicalize(&attachment)
            .with_context(|| format!("Reading RTFD attachment {name}"))?;
        ensure!(
            resolved.parent() == Some(root.as_path()),
            "An RTFD attachment points outside its bundle."
        );
        let image = read_document(&resolved)?;
        let kind = if image.starts_with(b"\x89PNG\r\n\x1a\n") {
            "pngblip"
        } else if image.starts_with(&[0xff, 0xd8, 0xff]) {
            "jpegblip"
        } else {
            bail!(
                "RTFD attachment '{name}' is not a PNG or JPEG image. The original bundle was preserved; this attachment needs a compatible converter."
            );
        };
        let group = &bytes[start..end];
        let dimension = |name: &[u8]| -> Option<i32> {
            let start = group.windows(name.len()).position(|word| word == name)? + name.len();
            let count = group[start..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            std::str::from_utf8(&group[start..start + count])
                .ok()?
                .parse::<i32>()
                .ok()?
                .checked_mul(20)
        };
        output.extend_from_slice(
            format!(
                "{{\\pict\\{kind}\\picwgoal{}\\pichgoal{} ",
                dimension(b"\\width").unwrap_or(2880),
                dimension(b"\\height").unwrap_or(2160)
            )
            .as_bytes(),
        );
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in image {
            output.push(HEX[usize::from(byte >> 4)]);
            output.push(HEX[usize::from(byte & 15)]);
        }
        output.push(b'}');
        copied = end;
    }
    output.extend_from_slice(&bytes[copied..]);
    ensure!(
        !has_external_graphics(&output),
        "The RTFD bundle contains unsupported external attachments; it was not modified."
    );
    Ok(output)
}

// Returns complete attachment groups, excluding escaped braces and binary picture data.
fn graphic_groups(bytes: &[u8]) -> Result<Vec<(usize, usize, usize, usize)>> {
    let mut stack: Vec<(usize, Option<(usize, usize)>)> = Vec::new();
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'{' => {
                stack.push((cursor, None));
                cursor += 1;
            }
            b'}' => {
                let (start, graphic) = stack
                    .pop()
                    .context("The RTF document has unbalanced groups")?;
                if let Some((name_start, name_end)) = graphic {
                    spans.push((start, cursor + 1, name_start, name_end));
                }
                cursor += 1;
            }
            b'\\' => {
                cursor += 1;
                if cursor >= bytes.len() {
                    bail!("The RTF document ends in an incomplete escape");
                }
                if !bytes[cursor].is_ascii_alphabetic() {
                    cursor += if bytes[cursor] == b'\'' { 3 } else { 1 };
                    continue;
                }
                let word_start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_alphabetic() {
                    cursor += 1;
                }
                let word = &bytes[word_start..cursor];
                let number_start = cursor;
                if bytes.get(cursor) == Some(&b'-') {
                    cursor += 1;
                }
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                let number_end = cursor;
                if bytes.get(cursor) == Some(&b' ') {
                    cursor += 1;
                }
                if word == b"NeXTGraphic" {
                    let name_start = cursor;
                    while cursor < bytes.len() && !matches!(bytes[cursor], b'\\' | b'}' | b'{') {
                        cursor += 1;
                    }
                    let group = stack
                        .last_mut()
                        .context("An RTFD attachment has no enclosing group")?;
                    ensure!(
                        group.1.is_none(),
                        "An RTFD group contains multiple unsupported attachments"
                    );
                    group.1 = Some((name_start, cursor));
                } else if word == b"bin" {
                    let count: usize = std::str::from_utf8(&bytes[number_start..number_end])?
                        .parse()
                        .context("Invalid RTF binary-data length")?;
                    cursor = cursor
                        .checked_add(count)
                        .context("RTF binary-data length overflow")?;
                    ensure!(cursor <= bytes.len(), "The RTF binary data is incomplete");
                }
            }
            _ => cursor += 1,
        }
    }
    ensure!(stack.is_empty(), "The RTF document has unbalanced groups");
    spans.sort_by_key(|span| span.0);
    ensure!(
        spans.windows(2).all(|pair| pair[0].1 <= pair[1].0),
        "Nested RTFD attachment groups are not supported"
    );
    Ok(spans)
}

pub fn supports_format(format: DocumentFormat) -> bool {
    !matches!(format, DocumentFormat::Doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn unicode_and_line_endings_round_trip_without_replacement() {
        let text = "Hello 🦀\nA\u{0301} العربية\n";
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Utf8Bom,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
        ] {
            for ending in [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr] {
                let encoded = encode_text(text, encoding, ending).expect("encoding valid Unicode");
                let (decoded, actual) =
                    decode_text(&encoded, None).expect("decoding valid Unicode");
                assert_eq!(
                    normalize_lines(&decoded),
                    text,
                    "encoding {encoding:?}, ending {ending:?}"
                );
                assert_eq!(actual, encoding);
            }
        }
    }

    #[test]
    fn legacy_encoding_refuses_lossy_save() {
        assert!(encode_text("Hello 🦀", TextEncoding::Windows1252, LineEnding::Lf).is_err());
        assert!(decode_text(&[0x80], None).is_err());
        assert_eq!(
            decode_text(&[0x80], Some(TextEncoding::Windows1252))
                .expect("explicit legacy encoding")
                .0,
            "€"
        );
    }

    #[test]
    fn rtf_escapes_braces_and_surrogates() {
        let output = content_as_rtf(&DocumentContent::Plain("{🦀}\n".into())).expect("valid text");
        let output = String::from_utf8(output).expect("RTF is ASCII");
        assert!(output.contains("\\{"));
        assert!(output.contains("\\u-10178?\\u-8832?"));
        assert!(output.contains("\\par"));
    }

    #[test]
    fn rtfd_parser_skips_escaped_braces_and_binary_pictures() {
        let rtf = br"{\rtf1 \{text\} {\pict\bin2 {}}{\NeXTGraphic image.png \width100\height100}}";
        let groups = graphic_groups(rtf).expect("valid attachment group");
        assert_eq!(groups.len(), 1);
        assert_eq!(
            std::str::from_utf8(&rtf[groups[0].2..groups[0].3])
                .expect("ASCII")
                .trim(),
            "image.png"
        );
    }

    #[test]
    fn plain_save_never_silently_discards_rich_content() {
        let temporary = TempDir::new().expect("temporary folder");
        let path = temporary.path().join("document.txt");
        let result = save(
            &path,
            DocumentFormat::PlainText,
            &DocumentContent::Rich(br"{\rtf1 styled}".to_vec()),
            TextEncoding::Utf8,
            LineEnding::Lf,
        );
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn fingerprint_detects_same_length_content_change() {
        let temporary = TempDir::new().expect("temporary folder");
        let path = temporary.path().join("document.txt");
        fs::write(&path, "first").expect("fixture");
        let before = fingerprint(&path).expect("first fingerprint");
        fs::write(&path, "other").expect("fixture edit");
        assert_ne!(before, fingerprint(&path).expect("second fingerprint"));
    }

    #[test]
    fn recovery_identifier_cannot_escape_its_folder() {
        for id in ["", "../document", "..\\document", "a/b", "a:b"] {
            assert!(recovery_path(id).is_err(), "identifier {id:?}");
        }
    }
    #[test]
    fn recovery_serialization_stops_at_its_size_bound() {
        let mut writer = LimitedWriter {
            inner: Vec::new(),
            remaining: 4,
        };
        writer
            .write_all(b"1234")
            .expect("writing within the snapshot limit");
        assert!(writer.write_all(b"5").is_err());
        assert_eq!(writer.inner, b"1234");
        assert_eq!(writer.remaining, 0);
    }
}
