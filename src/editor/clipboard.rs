//! Native rich clipboard adapters, adapted from WordCraft at 729cd0a5.
//! See assets/licenses/wordcraft/LICENSE-MIT.txt for the upstream notice.

use super::Editor;
use anyhow::{Context as _, Result, bail, ensure};
use base64::Engine as _;
use serde_json::json;
#[cfg(target_os = "windows")]
use std::io::Cursor;
use std::{cell::RefCell, io::Read as _, path::PathBuf};

const MAX_BYTES: usize = 16 << 20;
const MAX_IMAGE_BYTES: usize = 64 << 20;
const MAX_IMAGE_PIXELS: u64 = 50_000_000;
const MAX_IMAGE_SIDE: u32 = 16_000;
#[cfg(all(unix, not(target_os = "macos")))]
const IMAGE_MIMES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/tiff",
];

/// Native clipboard representations, with encoded images retained without resource fetching.
#[derive(Debug, Default)]
pub struct ClipboardContent {
    pub text: Option<String>,
    pub html: Option<String>,
    pub rtf: Option<Vec<u8>>,
    pub image: Option<Vec<u8>>,
    pub files: Vec<PathBuf>,
}

#[cfg(any(not(target_os = "windows"), test))]
fn utf8(bytes: Vec<u8>) -> Result<String> {
    let end = bytes
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |end| end + 1);
    let text = std::str::from_utf8(&bytes[..end]).context("Clipboard text is not valid UTF-8")?;
    ensure!(
        !text.contains('\0'),
        "Clipboard text contains an embedded NUL."
    );
    Ok(text.to_owned())
}

/// Reads supported formats from the native clipboard with bounded payloads.
pub fn read() -> Result<ClipboardContent> {
    platform::read()
}

/// Writes text, HTML and RTF together, retaining clipboard ownership where required.
pub fn write(text: &str, html: &str, rtf: &[u8]) -> Result<()> {
    ensure!(
        text.len() <= MAX_BYTES && html.len() <= MAX_BYTES && rtf.len() <= MAX_BYTES,
        "The selected clipboard content exceeds the 16 MiB safety limit."
    );
    ensure!(
        !text.contains('\0') && !html.contains('\0') && !rtf.contains(&0),
        "Clipboard content contains an embedded NUL."
    );
    platform::write(text, html, rtf)
}

thread_local! {
    // One selected fragment across document tabs; native formats must still match before reuse.
    static COPIED: RefCell<Option<(ClipboardContent, wordcraft_doc::Document)>> = const { RefCell::new(None) };
}

fn same_copy(current: &ClipboardContent, copied: &ClipboardContent) -> bool {
    current.text == copied.text
        && (current
            .rtf
            .as_ref()
            .is_some_and(|rtf| copied.rtf.as_ref() == Some(rtf))
            || current
                .html
                .as_ref()
                .is_some_and(|html| copied.html.as_ref() == Some(html)))
}

/// Copies the selected fragment; a failed clipboard write never deletes a cut selection.
pub fn copy(editor: &Editor, cut: bool) -> Result<()> {
    ensure!(
        !cut || !editor.is_read_only(),
        "This document is read-only."
    );
    let copied = editor.run_command("edit.copy", &json!({}))?;
    let text = copied["text"].as_str().unwrap_or("");
    let html = copied["html"].as_str().unwrap_or("");
    let rtf = copied["rtf"].as_str().unwrap_or("");
    if text.is_empty() && html.is_empty() && rtf.is_empty() {
        return Ok(());
    }
    write(text, html, rtf.as_bytes())?;
    let fragment = editor
        .session()
        .clipboard
        .as_ref()
        .map(|fragment| fragment.to_document());
    COPIED.with_borrow_mut(|slot| {
        *slot = fragment.map(|document| {
            (
                ClipboardContent {
                    text: Some(text.to_owned()),
                    html: Some(html.to_owned()),
                    rtf: Some(rtf.as_bytes().to_vec()),
                    ..Default::default()
                },
                document,
            )
        });
    });
    if cut {
        editor.run_command("edit.cut", &json!({}))?;
    }
    Ok(())
}

/// Pastes all supported rich data as one engine transaction, or only plain text when requested.
pub fn paste(editor: &Editor, plain: bool) -> Result<()> {
    ensure!(!editor.is_read_only(), "This document is read-only.");
    if plain || editor.is_plain() {
        let text = platform::text()?.context("The clipboard has no plain text to paste.")?;
        editor.run_command("edit.pasteText", &json!({"text": text}))?;
        editor.note_pasted();
        return Ok(());
    }
    let content = read()?;
    let copied = COPIED.with_borrow(|slot| {
        slot.as_ref()
            .filter(|(copied, _)| same_copy(&content, copied))
            .map(|(_, document)| document.clone())
    });
    if let Some(document) = copied {
        editor.paste_document(document)?;
    } else if !content.files.is_empty() {
        paste_files(editor, &content.files)?;
    } else if let Some(rtf) = content.rtf {
        editor.insert_rtf(&rtf)?;
    } else if let Some(html) = content.html {
        editor.insert_html(&html)?;
    } else if let Some(image) = content.image {
        paste_images(editor, vec![image])?;
    } else if let Some(text) = content.text {
        editor.run_command("edit.pasteText", &json!({"text": text}))?;
    } else {
        bail!("The clipboard has no supported text, rich text, or image.");
    }
    editor.note_pasted();
    Ok(())
}

/// Pastes HTML explicitly; CF_HTML selection ranges are decoded before import on Windows.
pub fn paste_html(editor: &Editor) -> Result<()> {
    ensure!(!editor.is_read_only(), "This document is read-only.");
    let html = platform::html()?.context("The clipboard has no HTML fragment.")?;
    if editor.is_plain() {
        bail!("Switch to a rich text document to paste HTML formatting.");
    }
    editor.insert_html(&html)?;
    editor.note_pasted();
    Ok(())
}

fn checked_image(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_IMAGE_BYTES,
        "The clipboard image exceeds the 64 MiB safety limit."
    );
    let probe =
        wordcraft_geom::picture::probe(bytes).context("reading the clipboard image header")?;
    ensure!(
        probe.width <= MAX_IMAGE_SIDE
            && probe.height <= MAX_IMAGE_SIDE
            && u64::from(probe.width) * u64::from(probe.height) <= MAX_IMAGE_PIXELS
            && probe.fits(wordcraft_geom::MAX_DECODED_BYTES),
        "The clipboard image is too large to paste safely."
    );
    Ok(())
}

fn paste_images(editor: &Editor, images: Vec<Vec<u8>>) -> Result<()> {
    // Staging validates every image first; the live document receives one undoable fragment.
    let mut staged = wordcraft_engine::Session::new(wordcraft_doc::Document::default());
    for bytes in images {
        checked_image(&bytes)?;
        staged.run(
            "insert.picture",
            &json!({"data": base64::engine::general_purpose::STANDARD.encode(bytes)}),
        )?;
    }
    editor.paste_document(staged.doc)
}

fn paste_files(editor: &Editor, files: &[PathBuf]) -> Result<()> {
    ensure!(files.len() <= 20, "Paste at most 20 image files at a time.");
    let mut images = Vec::with_capacity(files.len());
    let mut total = 0usize;
    for path in files {
        // A copied local image is explicit user input; HTML/RTF resource URLs are never opened.
        let name = path.to_string_lossy();
        ensure!(
            !name.starts_with("\\\\") && !name.starts_with("//"),
            "Network clipboard files are not opened."
        );
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        ensure!(
            ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"]
                .iter()
                .any(|ext| ext.eq_ignore_ascii_case(extension)),
            "Only copied local image files can be pasted into a document."
        );
        let metadata = std::fs::symlink_metadata(path).context("reading the copied image file")?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Clipboard images must be regular files, not folders or links."
        );
        ensure!(
            metadata.len() <= MAX_IMAGE_BYTES as u64,
            "The copied image file exceeds the 64 MiB safety limit."
        );
        let file = std::fs::File::open(path).context("opening the copied image file")?;
        let mut bytes = Vec::new();
        file.take(MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .context("reading the copied image file")?;
        total = total
            .checked_add(bytes.len())
            .context("Clipboard image size overflow")?;
        ensure!(
            total <= MAX_IMAGE_BYTES,
            "The copied images together exceed the 64 MiB safety limit."
        );
        checked_image(&bytes)?;
        images.push(bytes);
    }
    paste_images(editor, images)
}

#[cfg(not(target_os = "windows"))]
fn write_arboard(text: &str, html: &str) -> Result<()> {
    thread_local! {
        // X11 serves selection requests until a later clipboard owner replaces this handle.
        static WRITER: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
    }
    WRITER.with(|writer| {
        let mut slot = writer
            .try_borrow_mut()
            .context("The clipboard is already being written")?;
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().context("opening the native clipboard")?);
        }
        let clipboard = slot.as_mut().context("opening the native clipboard")?;
        if html.is_empty() {
            clipboard.set_text(text)
        } else {
            clipboard.set().html(html, Some(text))
        }
        .context("writing the native clipboard")
    })
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use clipboard_win::{Clipboard, raw};

    fn bytes(format: u32, cap: usize) -> Result<Option<Vec<u8>>> {
        if !raw::is_format_avail(format) {
            return Ok(None);
        }
        let size = raw::size(format)
            .context("The clipboard format has no readable data")?
            .get();
        ensure!(size <= cap, "Clipboard content exceeds its safety limit.");
        let mut bytes = Vec::new();
        raw::get_vec(format, &mut bytes)
            .map_err(|error| anyhow::anyhow!("reading the native clipboard: {error}"))?;
        ensure!(
            bytes.len() <= cap,
            "Clipboard content exceeds its safety limit."
        );
        Ok(Some(bytes))
    }
    fn registered(name: &str) -> Result<u32> {
        raw::register_format(name)
            .map(|format| format.get())
            .context("registering a native clipboard format")
    }

    fn read_text() -> Result<Option<String>> {
        if let Some(bytes) = bytes(13, MAX_BYTES * 2)? {
            ensure!(
                bytes.len() % 2 == 0,
                "Clipboard text has a truncated UTF-16 code unit."
            );
            let mut units: Vec<_> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            while units.last() == Some(&0) {
                units.pop();
            }
            ensure!(
                !units.contains(&0),
                "Clipboard text contains an embedded NUL."
            );
            let text = String::from_utf16(&units).context("Clipboard text is not valid UTF-16")?;
            ensure!(
                text.len() <= MAX_BYTES,
                "Clipboard text exceeds the 16 MiB safety limit."
            );
            return Ok(Some(text));
        }
        Ok(None)
    }
    pub fn text() -> Result<Option<String>> {
        let _open = Clipboard::new_attempts(10)
            .map_err(|error| anyhow::anyhow!("opening the native clipboard: {error}"))?;
        read_text()
    }

    pub fn html() -> Result<Option<String>> {
        let _open = Clipboard::new_attempts(10)
            .map_err(|error| anyhow::anyhow!("opening the native clipboard: {error}"))?;
        bytes(registered("HTML Format")?, MAX_BYTES)?
            .map(|bytes| crate::clipboard_html::decode(&bytes))
            .transpose()
    }

    pub fn read() -> Result<ClipboardContent> {
        let _open = Clipboard::new_attempts(10)
            .map_err(|error| anyhow::anyhow!("opening the native clipboard: {error}"))?;
        let mut content = ClipboardContent {
            text: read_text()?,
            ..Default::default()
        };
        if let Some(bytes) = bytes(registered("HTML Format")?, MAX_BYTES)? {
            content.html = Some(crate::clipboard_html::decode(&bytes)?);
        }
        content.rtf = bytes(registered("Rich Text Format")?, MAX_BYTES)?.map(|mut bytes| {
            while bytes.last() == Some(&0) {
                bytes.pop();
            }
            bytes
        });
        if let Some(png) = bytes(registered("PNG")?, MAX_IMAGE_BYTES)? {
            checked_image(&png)?;
            content.image = Some(png);
        } else if let Some(dib) = bytes(17, MAX_IMAGE_BYTES)?.or(bytes(8, MAX_IMAGE_BYTES)?) {
            let probe =
                wordcraft_geom::picture::probe_dib(dib).context("reading the clipboard bitmap")?;
            ensure!(
                probe.width <= MAX_IMAGE_SIDE
                    && probe.height <= MAX_IMAGE_SIDE
                    && u64::from(probe.width) * u64::from(probe.height) <= MAX_IMAGE_PIXELS,
                "The clipboard bitmap is too large to paste safely."
            );
            let image = probe
                .decode(wordcraft_geom::MAX_DECODED_BYTES)
                .context("decoding the clipboard bitmap")?;
            let mut png = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
                .context("encoding the clipboard bitmap")?;
            content.image = Some(png);
        }
        if raw::is_format_avail(15) {
            ensure!(
                raw::size(15).is_some_and(|size| size.get() <= 64 << 10),
                "The clipboard file list is too large."
            );
            raw::get_file_list_path(&mut content.files)
                .map_err(|error| anyhow::anyhow!("reading copied file paths: {error}"))?;
        }
        Ok(content)
    }

    pub fn write(text: &str, html: &str, rtf: &[u8]) -> Result<()> {
        let html_format = registered("HTML Format")?;
        let rtf_format = registered("Rich Text Format")?;
        let _open = Clipboard::new_attempts(10)
            .map_err(|error| anyhow::anyhow!("opening the native clipboard: {error}"))?;
        raw::set_string(text).map_err(|error| anyhow::anyhow!("copying selected text: {error}"))?;
        if !html.is_empty() {
            raw::set_html_with(html_format, html, clipboard_win::options::NoClear)
                .map_err(|error| anyhow::anyhow!("copying selected HTML: {error}"))?;
        }
        if !rtf.is_empty() {
            let mut terminated = rtf.to_vec();
            terminated.push(0);
            raw::set_without_clear(rtf_format, &terminated)
                .map_err(|error| anyhow::anyhow!("copying selected RTF: {error}"))?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSData, NSString};

    fn bytes(board: &NSPasteboard, kind: &str, cap: usize) -> Result<Option<Vec<u8>>> {
        let Some(data) = board.dataForType(&NSString::from_str(kind)) else {
            return Ok(None);
        };
        ensure!(
            data.len() <= cap,
            "Clipboard content exceeds its safety limit."
        );
        Ok(Some(data.to_vec()))
    }
    pub fn text() -> Result<Option<String>> {
        bytes(
            &NSPasteboard::generalPasteboard(),
            "public.utf8-plain-text",
            MAX_BYTES,
        )?
        .map(utf8)
        .transpose()
    }
    pub fn html() -> Result<Option<String>> {
        bytes(&NSPasteboard::generalPasteboard(), "public.html", MAX_BYTES)?
            .map(utf8)
            .transpose()
    }
    pub fn read() -> Result<ClipboardContent> {
        let board = NSPasteboard::generalPasteboard();
        let mut content = ClipboardContent {
            text: bytes(&board, "public.utf8-plain-text", MAX_BYTES)?
                .map(utf8)
                .transpose()?,
            html: bytes(&board, "public.html", MAX_BYTES)?
                .map(utf8)
                .transpose()?,
            rtf: bytes(&board, "public.rtf", MAX_BYTES)?,
            ..Default::default()
        };
        for kind in ["public.png", "public.tiff", "public.jpeg"] {
            if let Some(image) = bytes(&board, kind, MAX_IMAGE_BYTES)? {
                checked_image(&image)?;
                content.image = Some(image);
                break;
            }
        }
        let mut clipboard = arboard::Clipboard::new().context("opening the native clipboard")?;
        content.files = clipboard.get().file_list().unwrap_or_default();
        ensure!(
            content.files.len() <= 20,
            "Paste at most 20 image files at a time."
        );
        Ok(content)
    }
    pub fn write(text: &str, html: &str, rtf: &[u8]) -> Result<()> {
        write_arboard(text, html)?;
        if !rtf.is_empty() {
            ensure!(
                NSPasteboard::generalPasteboard().setData_forType(
                    Some(&NSData::with_bytes(rtf)),
                    &NSString::from_str("public.rtf")
                ),
                "The selected RTF could not be copied."
            );
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use super::*;
    use wl_clipboard_rs::paste::{
        ClipboardType, MimeType, Seat, get_contents, get_mime_types_ordered,
    };
    fn wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
    }

    fn bytes(mimes: &[&str], cap: usize) -> Result<Option<Vec<u8>>> {
        if !wayland() {
            return Ok(x11::read(mimes, cap));
        }
        let offered = get_mime_types_ordered(ClipboardType::Regular, Seat::Unspecified)
            .context("reading Wayland clipboard formats")?;
        let Some(mime) = mimes
            .iter()
            .find(|mime| offered.iter().any(|offered| offered == **mime))
        else {
            return Ok(None);
        };
        let (pipe, _) = get_contents(
            ClipboardType::Regular,
            Seat::Unspecified,
            MimeType::Specific(mime),
        )
        .context("reading the Wayland clipboard")?;
        let mut bytes = Vec::new();
        pipe.take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .context("reading clipboard data")?;
        ensure!(
            bytes.len() <= cap,
            "Clipboard content exceeds its safety limit."
        );
        Ok(Some(bytes))
    }
    pub fn text() -> Result<Option<String>> {
        bytes(
            &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"],
            MAX_BYTES,
        )?
        .map(utf8)
        .transpose()
    }
    pub fn html() -> Result<Option<String>> {
        bytes(&["text/html"], MAX_BYTES)?.map(utf8).transpose()
    }
    pub fn read() -> Result<ClipboardContent> {
        let mut content = ClipboardContent {
            text: bytes(
                &["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"],
                MAX_BYTES,
            )?
            .map(utf8)
            .transpose()?,
            html: bytes(&["text/html"], MAX_BYTES)?.map(utf8).transpose()?,
            rtf: bytes(&["text/rtf", "application/rtf", "text/richtext"], MAX_BYTES)?,
            ..Default::default()
        };
        for mime in IMAGE_MIMES {
            if let Some(image) = bytes(&[mime], MAX_IMAGE_BYTES)? {
                checked_image(&image)?;
                content.image = Some(image);
                break;
            }
        }
        let mut clipboard = arboard::Clipboard::new().context("opening the native clipboard")?;
        content.files = clipboard.get().file_list().unwrap_or_default();
        ensure!(
            content.files.len() <= 20,
            "Paste at most 20 image files at a time."
        );
        Ok(content)
    }
    pub fn write(text: &str, html: &str, rtf: &[u8]) -> Result<()> {
        if !wayland() {
            // shortcut: arboard serves text and HTML on X11, add a custom selection owner if an RTF-only X11 peer is required.
            return write_arboard(text, html);
        }
        use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
        let offer = |mime: &str, data: &[u8]| MimeSource {
            source: Source::Bytes(data.into()),
            mime_type: MimeType::Specific(mime.into()),
        };
        let mut sources = vec![offer("text/plain;charset=utf-8", text.as_bytes())];
        if !html.is_empty() {
            sources.push(offer("text/html", html.as_bytes()));
        }
        if !rtf.is_empty() {
            sources.push(offer("text/rtf", rtf));
        }
        Options::new()
            .copy_multi(sources)
            .context("writing the native Wayland clipboard")
    }
    fn complete(len: usize, bytes_after: u32, cap: usize) -> bool {
        bytes_after == 0 && len <= cap
    }
    mod x11 {
        use std::time::{Duration, Instant};

        use x11rb::connection::Connection;
        use x11rb::protocol::Event;
        use x11rb::protocol::xproto::{
            Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, GetPropertyReply, Window,
            WindowClass,
        };
        use x11rb::rust_connection::RustConnection;

        /// How long a paste waits for the selection's owner, in all.
        const TIMEOUT: Duration = Duration::from_secs(1);
        /// Most targets looked at.
        const MAX_TARGETS: usize = 128;

        struct Reader {
            conn: RustConnection,
            win: Window,
            clipboard: Atom,
            prop: Atom,
            deadline: Instant,
        }

        fn atom(conn: &RustConnection, name: &str) -> Option<Atom> {
            conn.intern_atom(false, name.as_bytes())
                .ok()?
                .reply()
                .ok()
                .map(|r| r.atom)
        }

        impl Reader {
            fn new() -> Option<Self> {
                let (conn, screen) = x11rb::connect(None).ok()?;
                let root = conn.setup().roots.get(screen)?.root;
                let win = conn.generate_id().ok()?;
                conn.create_window(
                    0,
                    win,
                    root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    WindowClass::INPUT_ONLY,
                    x11rb::COPY_FROM_PARENT,
                    &CreateWindowAux::new(),
                )
                .ok()?;
                let clipboard = atom(&conn, "CLIPBOARD")?;
                let prop = atom(&conn, "EXEDITOR_PASTE")?;
                Some(Self {
                    conn,
                    win,
                    clipboard,
                    prop,
                    deadline: Instant::now() + TIMEOUT,
                })
            }

            /// The selection converted to `target`, at most `cap` bytes; `None` when the owner
            /// refuses or doesn't answer in time. A longer value is `None` when `whole`, else
            /// its first `cap` bytes.
            fn convert(&self, target: Atom, cap: usize, whole: bool) -> Option<GetPropertyReply> {
                self.conn
                    .convert_selection(
                        self.win,
                        self.clipboard,
                        target,
                        self.prop,
                        x11rb::CURRENT_TIME,
                    )
                    .ok()?;
                self.conn.flush().ok()?;
                loop {
                    // Checked every time round, so a stream of other events can't keep us here.
                    if Instant::now() >= self.deadline {
                        return None;
                    }
                    match self.conn.poll_for_event().ok()? {
                        Some(Event::SelectionNotify(e)) if e.requestor == self.win => {
                            if e.property == x11rb::NONE {
                                return None;
                            }
                            break;
                        }
                        Some(_) => {}
                        None => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
                // In 32-bit units; for a whole value one more than the cap, so a larger one shows
                // as `bytes_after`.
                let units = u32::try_from(cap / 4 + usize::from(whole)).unwrap_or(u32::MAX);
                let r = self
                    .conn
                    .get_property(true, self.win, self.prop, AtomEnum::ANY, 0, units)
                    .ok()?
                    .reply()
                    .ok()?;
                (!whole || super::complete(r.value.len(), r.bytes_after, cap)).then_some(r)
            }
        }

        impl Drop for Reader {
            fn drop(&mut self) {
                let _ = self.conn.destroy_window(self.win);
                let _ = self.conn.flush();
            }
        }

        /// The CLIPBOARD selection's RTF, at most `cap` bytes. Copies too large to send at once
        /// arrive in pieces (INCR), which aren't read: `None` then.
        pub fn read(mimes: &[&str], cap: usize) -> Option<Vec<u8>> {
            let r = Reader::new()?;
            let targets = r.convert(
                atom(&r.conn, "TARGETS")?,
                MAX_TARGETS.saturating_mul(4),
                false,
            )?;
            let cookies: Vec<_> = targets
                .value32()?
                .take(MAX_TARGETS)
                .filter_map(|a| r.conn.get_atom_name(a).ok())
                .collect();
            let names: Vec<String> = cookies
                .into_iter()
                .filter_map(|c| c.reply().ok())
                .map(|n| String::from_utf8_lossy(&n.name).into_owned())
                .collect();
            let mime = mimes
                .iter()
                .find(|mime| names.iter().any(|name| name == **mime))?;
            let target = atom(&r.conn, mime)?;
            let incr = atom(&r.conn, "INCR")?;
            let reply = r.convert(target, cap, true)?;
            (reply.type_ != incr).then_some(reply.value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_clipboard_keeps_unicode_and_rejects_embedded_nuls() {
        assert_eq!(
            utf8("café 👩🏽‍💻\0\0".as_bytes().to_vec()).expect("valid text"),
            "café 👩🏽‍💻"
        );
        assert!(utf8(b"one\0two".to_vec()).is_err());
        assert!(utf8(vec![0xff]).is_err());
    }
    #[test]
    fn cached_fragment_requires_matching_native_rich_data() {
        let copied = ClipboardContent {
            text: Some("hello".into()),
            html: Some("<b>hello</b>".into()),
            rtf: Some(b"{\\rtf1 hello}".to_vec()),
            ..Default::default()
        };
        let mut current = ClipboardContent {
            text: Some("hello".into()),
            html: Some("<i>hello</i>".into()),
            ..Default::default()
        };
        assert!(
            !same_copy(&current, &copied),
            "same plain text must not reuse stale formatting"
        );
        current.rtf.clone_from(&copied.rtf);
        assert!(
            same_copy(&current, &copied),
            "one identical native rich representation identifies the selected fragment"
        );
        current.text = Some("different".into());
        assert!(!same_copy(&current, &copied));
    }
    #[test]
    fn rejects_oversized_images_from_the_header_without_pixel_allocation() {
        // A BMP header advertising far more pixels than a clipboard paste accepts.
        let mut bmp = vec![0u8; 54];
        bmp[..2].copy_from_slice(b"BM");
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&20000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&20000i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        assert!(checked_image(&bmp).is_err());
    }
}
