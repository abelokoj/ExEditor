//! Bounded, offline document codecs. NSH owns the live editing model.
use crate::{document::DocumentFormat, editor::PageSettings};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use scraper::{ElementRef, Html};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Read as _},
    path::{Component, Path},
};

mod legacy;
mod office;
mod rtf;

const MAX_DOCUMENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_HTML_BYTES: usize = 128 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_NODES: usize = 1_000_000;
const MAX_DEPTH: usize = 128;

/// Application-owned interchange snapshot; never the live editor or recovery model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RichDocument {
    pub html: String,
    #[serde(default)]
    pub page: PageSettings,
}
impl RichDocument {
    pub fn from_text(text: &str) -> Self {
        Self {
            html: format!(
                "<p style=\"white-space:pre-wrap\">{}</p>",
                escape(text).replace('\n', "<br>")
            ),
            page: PageSettings::default(),
        }
    }
}

/// Opens supported formats without invoking an external application.
pub(crate) fn import(path: &Path, format: DocumentFormat) -> Result<RichDocument> {
    let bytes = read_bounded(path, MAX_DOCUMENT_BYTES)?;
    match format {
        DocumentFormat::Html => Ok(RichDocument {
            html: sanitize_html(
                &decode_html(&bytes),
                Some(path.parent().unwrap_or(Path::new("."))),
                MAX_HTML_BYTES,
            )?,
            page: PageSettings::default(),
        }),
        DocumentFormat::Docx => office::read_docx(&bytes),
        DocumentFormat::Odt => office::read_odt(&bytes),
        DocumentFormat::Doc => legacy::read(&bytes),
        DocumentFormat::Rtf => import_rtf(&bytes),
        _ => bail!("This format is opened directly by the document editor"),
    }
}

/// Encodes an interchange snapshot without replacing the native editing session.
pub(crate) fn export_document(document: &RichDocument, format: DocumentFormat) -> Result<Vec<u8>> {
    match format {
        DocumentFormat::Html => Ok(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{}</body></html>",
            safe_native_html(&document.html)?
        )
        .into_bytes()),
        DocumentFormat::Docx => office::write_docx(document),
        DocumentFormat::Odt => office::write_odt(document),
        DocumentFormat::Rtf => export_rtf(document),
        DocumentFormat::Doc => bail!(
            "Legacy Word (.doc) export is unavailable. Choose Word (.docx) or Rich Text (.rtf); the original has not been replaced."
        ),
        _ => bail!("This format is saved directly by the document editor"),
    }
}
pub(crate) fn export(bytes: &[u8], format: DocumentFormat) -> Result<Vec<u8>> {
    export_document(&import_rtf(bytes)?, format)
}
pub(crate) fn import_rtf(bytes: &[u8]) -> Result<RichDocument> {
    rtf::read(bytes)
}
pub(crate) fn export_rtf(document: &RichDocument) -> Result<Vec<u8>> {
    rtf::write(document)
}

/// Renders Markdown locally; media can only come from beneath the document directory.
pub(crate) fn markdown_document(source: &str, directory: &Path) -> Result<RichDocument> {
    use pulldown_cmark::{Options, Parser, html};
    ensure!(
        source.len() <= MAX_HTML_BYTES,
        "The Markdown document exceeds its size limit."
    );
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut markup = String::new();
    html::push_html(&mut markup, Parser::new_ext(source, options));
    Ok(RichDocument {
        html: sanitize_html(&markup, Some(directory), MAX_HTML_BYTES)?,
        page: PageSettings::default(),
    })
}

/// Clipboard markup cannot read neighbouring files or download remote media.
pub(crate) fn clipboard_document(source: &str) -> Result<RichDocument> {
    validate_clipboard_html(source)?;
    let html = sanitize_html(source, None, crate::clipboard_html::MAX_BYTES)?;
    let parsed = Html::parse_fragment(&html);
    ensure!(
        !parsed.root_element().text().all(|s| s.trim().is_empty())
            || html.contains("<img ")
            || html.contains("<table"),
        "Clipboard HTML contains no supported text, images or tables."
    );
    Ok(RichDocument {
        html,
        page: PageSettings::default(),
    })
}

pub(crate) fn validate_clipboard_html(source: &str) -> Result<()> {
    validate_html(source, crate::clipboard_html::MAX_BYTES)
}

/// Sanitizes native snapshots including empty documents, without a filesystem resolver.
pub(crate) fn safe_native_html(source: &str) -> Result<String> {
    sanitize_html(source, None, MAX_HTML_BYTES)
}

/// Allows web/email links and relative/bookmark navigation, never executable or file URLs.
pub(crate) fn is_safe_href(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty()
        || href.chars().any(|c| c.is_control() || c == '\\')
        || href.starts_with("//")
    {
        return false;
    }
    match href.split_once(':') {
        Some((scheme, _)) => matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto" | "tel"
        ),
        None => true,
    }
}

fn validate_html(source: &str, limit: usize) -> Result<()> {
    ensure!(source.len() <= limit, "HTML exceeds its safety size limit.");
    ensure!(!source.contains('\0'), "HTML contains an embedded NUL.");
    let parsed = Html::parse_fragment(source);
    for (count, node) in parsed.tree.root().descendants().enumerate() {
        ensure!(count < MAX_NODES, "HTML contains too many elements.");
        ensure!(
            node.ancestors().take(MAX_DEPTH + 2).count() <= MAX_DEPTH,
            "HTML is nested too deeply."
        );
    }
    Ok(())
}

fn sanitize_html(source: &str, directory: Option<&Path>, limit: usize) -> Result<String> {
    validate_html(source, limit)?;
    let parsed = Html::parse_fragment(source);
    let mut out = String::new();
    let mut image_bytes = 0;
    sanitize_children(parsed.root_element(), directory, &mut image_bytes, &mut out)?;
    ensure!(
        out.len() <= MAX_HTML_BYTES,
        "The sanitized document exceeds its size limit."
    );
    if out.trim().is_empty() {
        out.push_str("<p></p>");
    }
    Ok(out)
}

fn sanitize_children(
    element: ElementRef<'_>,
    directory: Option<&Path>,
    image_bytes: &mut u64,
    out: &mut String,
) -> Result<()> {
    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            out.push_str(&escape(text));
            continue;
        }
        let Some(element) = ElementRef::wrap(child) else {
            continue;
        };
        let tag = element.value().name();
        if matches!(
            tag,
            "script"
                | "style"
                | "iframe"
                | "object"
                | "embed"
                | "svg"
                | "math"
                | "head"
                | "template"
                | "noscript"
                | "audio"
                | "video"
                | "canvas"
        ) {
            continue;
        }
        let allowed = matches!(
            tag,
            "p" | "div"
                | "span"
                | "br"
                | "hr"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "b"
                | "strong"
                | "i"
                | "em"
                | "u"
                | "s"
                | "strike"
                | "del"
                | "sub"
                | "sup"
                | "code"
                | "pre"
                | "blockquote"
                | "ul"
                | "ol"
                | "li"
                | "table"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
                | "a"
                | "img"
        );
        if !allowed {
            sanitize_children(element, directory, image_bytes, out)?;
            continue;
        }
        if tag == "img" {
            let src = element.value().attr("src").unwrap_or("");
            let bytes = image_data(src).or_else(|| directory.and_then(|dir| local_image(dir, src)));
            if let Some(bytes) = bytes {
                if let Some(uri) = image_uri(&bytes) {
                    *image_bytes += bytes.len() as u64;
                    ensure!(
                        *image_bytes <= MAX_HTML_BYTES as u64,
                        "Embedded images exceed the document resource limit."
                    );
                    out.push_str("<img src=\"");
                    out.push_str(&uri);
                    out.push('"');
                    for attr in ["alt", "title", "width", "height"] {
                        if let Some(value) = element.value().attr(attr) {
                            if matches!(attr, "width" | "height")
                                && !value.parse::<u32>().is_ok_and(|n| (1..=32768).contains(&n))
                            {
                                continue;
                            }
                            attribute(out, attr, value);
                        }
                    }
                    out.push('>');
                    continue;
                }
            }
            if let Some(alt) = element.value().attr("alt") {
                out.push_str(&escape(alt));
            }
            continue;
        }
        out.push('<');
        out.push_str(tag);
        if let Some(value) = element.value().attr("style") {
            let value = safe_style(value);
            if !value.is_empty() {
                attribute(out, "style", &value);
            }
        }
        for attr in ["title", "colspan", "rowspan", "start", "dir", "id"] {
            if let Some(value) = element.value().attr(attr) {
                if matches!(attr, "colspan" | "rowspan" | "start")
                    && !value.parse::<u32>().is_ok_and(|n| (1..=1000).contains(&n))
                {
                    continue;
                }
                if attr == "dir" && !matches!(value, "ltr" | "rtl" | "auto") {
                    continue;
                }
                attribute(out, attr, value);
            }
        }
        if tag == "a" {
            if let Some(href) = element
                .value()
                .attr("href")
                .filter(|href| is_safe_href(href))
            {
                attribute(out, "href", href);
            }
        }
        out.push('>');
        if !matches!(tag, "br" | "hr") {
            sanitize_children(element, directory, image_bytes, out)?;
            out.push_str("</");
            out.push_str(tag);
            out.push('>');
        }
        ensure!(
            out.len() <= MAX_HTML_BYTES,
            "The document exceeds its expanded size limit."
        );
    }
    Ok(())
}
fn safe_style(source: &str) -> String {
    source
        .split(';')
        .filter_map(|declaration| {
            let (key, value) = declaration.split_once(':')?;
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            if value.len() > 256
                || !matches!(
                    key.as_str(),
                    "font-family"
                        | "font-size"
                        | "font-weight"
                        | "font-style"
                        | "text-decoration"
                        | "color"
                        | "background-color"
                        | "text-align"
                        | "line-height"
                        | "margin-left"
                        | "text-indent"
                        | "direction"
                        | "vertical-align"
                        | "white-space"
                )
            {
                return None;
            }
            let valid = if matches!(key.as_str(), "color" | "background-color") {
                value.parse::<csscolorparser::Color>().is_ok()
            } else {
                !value.is_empty()
                    && value
                        .chars()
                        .all(|c| c.is_alphanumeric() || " #.,%'\"-".contains(c))
            };
            valid.then(|| format!("{key}:{value}"))
        })
        .collect::<Vec<_>>()
        .join(";")
}
fn attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&escape(value));
    out.push('"');
}
pub(crate) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn decode_html(bytes: &[u8]) -> String {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode(&bytes[bom..]).0.into_owned();
    }
    if let Ok(source) = std::str::from_utf8(bytes) {
        return source.to_owned();
    }
    encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("Reading {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "The document is not a regular file.");
    ensure!(
        metadata.len() <= limit,
        "The document exceeds its size limit."
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "The document exceeds its size limit."
    );
    Ok(bytes)
}
fn local_image(directory: &Path, source: &str) -> Option<Vec<u8>> {
    let decoded = percent_encoding::percent_decode_str(source)
        .decode_utf8()
        .ok()?;
    if decoded.contains(':') || decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    let relative = Path::new(decoded.as_ref());
    if relative.is_absolute()
        || relative
            .components()
            .any(|p| !matches!(p, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let directory = directory.canonicalize().ok()?;
    let path = directory.join(relative).canonicalize().ok()?;
    if !path.starts_with(&directory) {
        return None;
    }
    let bytes = read_bounded(&path, MAX_IMAGE_BYTES).ok()?;
    checked_image(&bytes).then_some(bytes)
}
fn checked_image(bytes: &[u8]) -> bool {
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return false;
    }
    let Ok(reader) = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return false;
    };
    let Ok((width, height)) = reader.into_dimensions() else {
        return false;
    };
    width > 0
        && height > 0
        && width <= 32768
        && height <= 32768
        && u64::from(width) * u64::from(height) <= 64_000_000
}
fn image_uri(bytes: &[u8]) -> Option<String> {
    if !checked_image(bytes) {
        return None;
    }
    let mime = match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::Bmp => "image/bmp",
        image::ImageFormat::Tiff => "image/tiff",
        _ => return None,
    };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
fn image_data(source: &str) -> Option<Vec<u8>> {
    let (header, encoded) = source.split_once(',')?;
    if !matches!(
        header.to_ascii_lowercase().as_str(),
        "data:image/png;base64"
            | "data:image/jpeg;base64"
            | "data:image/gif;base64"
            | "data:image/bmp;base64"
            | "data:image/tiff;base64"
    ) || encoded.len() as u64 > MAX_IMAGE_BYTES * 4 / 3 + 4
    {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    checked_image(&bytes).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_preserves_unicode_runs_tables_and_confined_media() {
        let root = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(root.path().join("picture.png"))
            .unwrap();
        let doc = markdown_document("# Heading\n\n**café 😀** [link](https://example.com)\n\n![picture](picture.png)\n\n| A | B |\n|---|---|\n| cell | value |", root.path()).unwrap();
        assert!(doc.html.contains("café 😀"));
        assert!(doc.html.contains("<strong>"));
        assert!(doc.html.contains("<table>"));
        assert!(doc.html.contains("data:image/png;base64,"));
        for source in [
            "../secret.png",
            "%2e%2e/secret.png",
            "https://example.com/a.png",
            "file:///C:/secret.png",
            "C:\\secret.png",
        ] {
            assert!(local_image(root.path(), source).is_none());
        }
    }
    #[test]
    fn clipboard_blocks_active_content_and_all_external_media() {
        let doc = clipboard_document("<p onclick='bad()'>Hello<script>bad()</script><a href='javascript:bad()'>world</a></p><img src='file:///private.png' alt='private image'>").unwrap();
        assert!(!doc.html.contains("script"));
        assert!(!doc.html.contains("onclick"));
        assert!(!doc.html.contains("file:"));
        assert!(!doc.html.contains("<img"));
        assert!(doc.html.contains("world"));
        assert!(clipboard_document("<b></b>").is_err());
        assert!(clipboard_document("a\0b").is_err());
        assert!(
            clipboard_document(&format!(
                "{}text{}",
                "<div>".repeat(140),
                "</div>".repeat(140)
            ))
            .is_err()
        );
        assert_eq!(safe_native_html("").unwrap(), "<p></p>");
        for href in [
            "javascript:bad()",
            "data:text/html,bad",
            "file:///secret",
            "\\\\server\\share",
            "//server/share",
            "java\nscript:bad()",
        ] {
            assert!(!is_safe_href(href));
        }
    }
    #[test]
    fn legacy_export_refuses_to_replace_the_original() {
        assert!(export_document(&RichDocument::from_text("keep me"), DocumentFormat::Doc).is_err());
    }
}
