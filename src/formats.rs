//! Offline document conversions through the same WordCraft model used by the editor.
use crate::document::DocumentFormat;
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::File,
    io::Read as _,
    path::{Component, Path},
    sync::Arc,
};
use wordcraft_doc::Document;

const MAX_DOCUMENT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 128 * 1024 * 1024;

/// Opens a supported document without flattening it through an intermediate RTF model.
pub(crate) fn import(path: &Path, format: DocumentFormat) -> Result<Document> {
    let bytes = read_bounded(path, MAX_DOCUMENT_BYTES)?;
    let mut document = match format {
        DocumentFormat::Html => {
            let source = wordcraft_formats::html::decode(&bytes);
            let directory = path.parent().unwrap_or(Path::new("."));
            wordcraft_formats::html::import_page(&source, &|src| local_image(directory, src))
        }
        DocumentFormat::Docx => {
            wordcraft_docx::read(&bytes).context("Reading the Word document")?
        }
        DocumentFormat::Doc => {
            wordcraft_docbin::read(&bytes).context("Reading the legacy Word document")?
        }
        DocumentFormat::Odt => {
            wordcraft_formats::odt::import(&bytes).map_err(anyhow::Error::msg)?
        }
        _ => bail!("This format is opened directly by the document editor"),
    };
    document.ensure_nonempty();
    Ok(document)
}

/// Saves the canonical document, retaining WordCraft's native DOCX/ODT structure.
pub(crate) fn export_document(document: &Document, format: DocumentFormat) -> Result<Vec<u8>> {
    match format {
        DocumentFormat::Html => Ok(wordcraft_formats::html::export(document).into_bytes()),
        DocumentFormat::Docx => {
            wordcraft_docx::write(document).context("Writing the Word document")
        }
        DocumentFormat::Odt => wordcraft_formats::odt::export(document).map_err(anyhow::Error::msg),
        DocumentFormat::Doc => bail!(
            "Legacy Word (.doc) export is unavailable. Choose Word (.docx) or Rich Text (.rtf); the original has not been replaced."
        ),
        _ => bail!("This format is saved directly by the document editor"),
    }
}

pub(crate) fn export(rtf: &[u8], format: DocumentFormat) -> Result<Vec<u8>> {
    let document = wordcraft_formats::rtf::import(rtf).map_err(anyhow::Error::msg)?;
    export_document(&document, format)
}

/// Renders Markdown locally; external images are never downloaded.
pub(crate) fn markdown_document(source: &str, directory: &Path) -> Result<Document> {
    use pulldown_cmark::{Options, Parser, html};
    ensure!(
        source.len() as u64 <= MAX_DOCUMENT_BYTES,
        "The Markdown document exceeds its size limit."
    );
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let mut markup = String::new();
    html::push_html(&mut markup, Parser::new_ext(source, options));
    let mut document =
        wordcraft_formats::html::import_page(&markup, &|src| local_image(directory, src));
    document.ensure_nonempty();
    Ok(document)
}

/// Clipboard markup has no permission to read neighbouring files or remote media.
pub(crate) fn clipboard_document(source: &str) -> Result<Document> {
    ensure!(
        source.len() <= crate::clipboard_html::MAX_BYTES,
        "Clipboard HTML exceeds the 16 MiB safety limit."
    );
    ensure!(
        !source.contains('\0'),
        "Clipboard HTML contains an embedded NUL."
    );
    let parsed = scraper::Html::parse_fragment(source);
    for (count, node) in parsed.tree.root().descendants().enumerate() {
        ensure!(
            count < 1_000_000,
            "Clipboard HTML contains too many elements."
        );
        ensure!(
            node.ancestors().take(130).count() <= 128,
            "Clipboard HTML is nested too deeply."
        );
    }
    let mut document = wordcraft_formats::html::import_page(source, &|_| None);
    document.ensure_nonempty();
    ensure!(
        !wordcraft_formats::txt::export(&document)
            .iter()
            .all(u8::is_ascii_whitespace)
            || !document.media.is_empty()
            || document
                .body
                .iter()
                .any(|block| matches!(block.as_ref(), wordcraft_doc::Block::Table(_))),
        "Clipboard HTML contains no supported text, images or tables."
    );
    Ok(document)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("Reading {}", path.display()))?;
    ensure!(
        file.metadata()?.is_file(),
        "The document is not a regular file."
    );
    ensure!(
        file.metadata()?.len() <= limit,
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

fn local_image(directory: &Path, source: &str) -> Option<Arc<Vec<u8>>> {
    let decoded = percent_encoding::percent_decode_str(source)
        .decode_utf8()
        .ok()?;
    if decoded.contains(':') || decoded.contains('\0') {
        return None;
    }
    let relative = Path::new(decoded.as_ref());
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    let directory = directory.canonicalize().ok()?;
    let path = directory.join(relative).canonicalize().ok()?;
    if !path.starts_with(&directory) {
        return None;
    }
    read_bounded(&path, MAX_IMAGE_BYTES).ok().map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_markdown_import_keeps_formatting_tables_and_local_media_in_the_native_model() {
        let root = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(root.path().join("picture.png"))
            .unwrap();
        let text = format!(
            "# Heading\n\n**bold café 😀** and [link](https://example.com)\n\n![picture](picture.png)\n\n| A | B |\n|---|---|\n| cell | value |\n\n{}",
            "Paragraph preserved in a long document.\n\n".repeat(700)
        );
        let doc = markdown_document(&text, root.path()).unwrap();
        assert!(
            doc.plain_text(wordcraft_doc::StoryRef::Body)
                .contains("bold café 😀")
        );
        assert!(doc.body.len() > 700);
        assert!(!doc.media.is_empty());
        assert!(
            doc.body
                .iter()
                .any(|block| matches!(&**block, wordcraft_doc::Block::Table(_)))
        );
        let html = wordcraft_formats::html::export(&doc);
        assert!(html.contains("https://example.com"));
        assert!(html.contains("picture") || html.contains("<img"));
    }

    #[test]
    fn html_and_docx_share_the_editor_model_and_keep_rich_runs() {
        let document = clipboard_document(
            "<p>plain <b>bold café 😀</b></p><table><tr><td>cell</td></tr></table>",
        )
        .unwrap();
        let bytes = export_document(&document, DocumentFormat::Docx).unwrap();
        let reloaded = wordcraft_docx::read(&bytes).unwrap();
        let html = wordcraft_formats::html::export(&reloaded);
        assert!(html.contains("bold café 😀"));
        assert!(html.contains("<table"));
        let rtf = wordcraft_formats::rtf::export(&reloaded);
        assert!(rtf.contains("\\b"));
    }

    #[test]
    fn local_images_cannot_escape_the_opened_documents_directory() {
        let root = tempfile::tempdir().unwrap();
        assert!(local_image(root.path(), "../secret.png").is_none());
        assert!(local_image(root.path(), "%2e%2e/secret.png").is_none());
        assert!(local_image(root.path(), "https://example.com/a.png").is_none());
        assert!(local_image(root.path(), "file:///C:/secret.png").is_none());
        assert!(local_image(root.path(), "C:\\secret.png").is_none());
    }

    #[test]
    fn clipboard_markup_rejects_empty_nul_and_deep_fragments() {
        assert!(clipboard_document("<b></b>").is_err());
        assert!(clipboard_document("a\0b").is_err());
        let markup = format!("{}text{}", "<div>".repeat(140), "</div>".repeat(140));
        assert!(clipboard_document(&markup).is_err());
        let document =
            clipboard_document("<p>Hello</p><img src='file:///private.png' alt='private image'>")
                .unwrap();
        assert!(document.media.is_empty());
    }

    #[test]
    fn legacy_doc_export_preserves_the_original_by_refusing_the_write() {
        assert!(export_document(&Document::from_text("keep me"), DocumentFormat::Doc).is_err());
    }
}
