//! Independent PDF export and native printer submission share one physical page layout.

use super::types::PageSettings;
use crate::formats::RichDocument;
use anyhow::{Context as _, Result, ensure};
use cosmic_text::FontSystem;
use std::{io::Write as _, path::Path};

mod layout;
mod markup;
mod render;

/// One configured native system print queue, including virtual PDF printers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Printer {
    pub name: String,
    pub is_default: bool,
}

/// Settings explicitly chosen in the editor's native print form.
#[derive(Clone, Debug)]
pub struct PrintOptions {
    pub printer: String,
    pub copies: u32,
    pub duplex: bool,
    pub grayscale: bool,
    pub page_range: Option<String>,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            printer: String::new(),
            copies: 1,
            duplex: false,
            grayscale: false,
            page_range: None,
        }
    }
}

/// Lists native spooler queues without invoking printer configuration dialogs.
///
/// # Errors
/// Returns an error when this platform has no native print backend.
pub fn printers() -> Result<Vec<Printer>> {
    ensure!(
        craft_print::backend().is_some(),
        "Native printing is unavailable on this platform; Export PDF remains available."
    );
    Ok(craft_print::printer_queues()
        .into_iter()
        .map(|printer| Printer {
            name: printer.name,
            is_default: printer.default,
        })
        .collect())
}

pub(super) fn validate_page_settings(page: &PageSettings) -> Result<()> {
    const MAX_TWIPS: i32 = 288_000;
    ensure!(
        (1..=MAX_TWIPS).contains(&page.width_twips) && (1..=MAX_TWIPS).contains(&page.height_twips),
        "Page dimensions must be positive and no larger than 200 inches."
    );
    ensure!(
        [
            page.margin_left,
            page.margin_right,
            page.margin_top,
            page.margin_bottom
        ]
        .iter()
        .all(|&margin| (0..=MAX_TWIPS).contains(&margin)),
        "Page margins must be nonnegative and no larger than 200 inches."
    );
    ensure!(
        page.margin_left + page.margin_right < page.width_twips
            && page.margin_top + page.margin_bottom < page.height_twips,
        "Page margins leave no room for document content."
    );
    Ok(())
}

/// Applies validated physical page geometry to an interchange snapshot.
///
/// # Errors
/// Invalid dimensions or margins leave the document unchanged.
#[cfg(test)]
pub fn apply_page_settings(doc: &mut RichDocument, page: &PageSettings) -> Result<()> {
    validate_page_settings(page)?;
    doc.page = page.clone();
    Ok(())
}

fn parse_page_range(spec: &str, total: usize) -> Result<Vec<usize>> {
    ensure!(total > 0, "The document has no printable pages.");
    ensure!(
        spec.len() <= 16_384,
        "The page range exceeds its size limit."
    );
    let mut pages = std::collections::BTreeSet::new();
    for item in spec.split(',').map(str::trim) {
        let number = |text: &str| -> Result<usize> {
            let page = text
                .trim()
                .parse::<usize>()
                .context("Use page numbers or ranges, such as 1-3,5")?;
            ensure!(
                (1..=total).contains(&page),
                "The page range exceeds the document's {total} pages."
            );
            Ok(page)
        };
        match item.to_ascii_lowercase().as_str() {
            "all" => pages.extend(0..total),
            "odd" => pages.extend((0..total).step_by(2)),
            "even" => pages.extend((1..total).step_by(2)),
            _ => {
                if let Some((start, end)) = item.split_once('-') {
                    let start = if start.trim().is_empty() {
                        1
                    } else {
                        number(start)?
                    };
                    let end = if end.trim().is_empty() {
                        total
                    } else {
                        number(end)?
                    };
                    ensure!(start <= end, "The page range is reversed.");
                    pages.extend(start - 1..end);
                } else {
                    pages.insert(number(item)? - 1);
                }
            }
        }
    }
    ensure!(!pages.is_empty(), "The page range selects no pages.");
    Ok(pages.into_iter().collect())
}

fn pdf(
    doc: &RichDocument,
    page: &PageSettings,
    links: bool,
    page_range: Option<&str>,
) -> Result<Vec<u8>> {
    validate_page_settings(page)?;
    let blocks = markup::parse(&doc.html).context("Preparing the native document for printing")?;
    let font_data = crate::fonts::export_font_data().context("Loading output fonts")?;
    let sources = font_data
        .into_iter()
        .map(|data| cosmic_text::fontdb::Source::Binary(std::sync::Arc::new(data)));
    let mut fonts = FontSystem::new_with_fonts(sources);
    fonts.db_mut().set_sans_serif_family("Inter");
    fonts
        .db_mut()
        .set_monospace_family(crate::fonts::default_plain_font());
    let width = (page.width_twips - page.margin_left - page.margin_right) as f32 / 20.;
    let height = (page.height_twips - page.margin_top - page.margin_bottom) as f32 / 20.;
    let bands =
        layout::typeset(&blocks, width, height, &mut fonts).context("Laying out print pages")?;
    let pages = layout::paginate(&bands, width, height)?;
    let selected = match page_range.filter(|spec| !spec.trim().is_empty()) {
        Some(spec) => parse_page_range(spec, pages.len())?,
        None => (0..pages.len()).collect(),
    };
    render::draw(&pages, &selected, page, &fonts, links)
}

/// Writes an independent, font-embedded PDF atomically without requiring any printer.
/// Call this blocking font discovery and rendering operation from a worker.
///
/// # Errors
/// Reports invalid geometry, unavailable glyphs, image/layout errors and output failures.
/// An existing output file is preserved unless the complete PDF is successfully saved.
pub fn export_pdf(doc: &RichDocument, path: &Path, page: &PageSettings) -> Result<()> {
    let bytes = pdf(doc, page, true, None)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(parent).context("Creating the PDF output")?;
    output.write_all(&bytes).context("Writing the PDF")?;
    output.as_file().sync_all().context("Saving the PDF")?;
    output
        .persist(path)
        .map_err(|error| error.error)
        .context("Replacing the PDF output")?;
    Ok(())
}

/// Sends the paginated PDF to a system printer using the confirmed print settings.
/// Call this blocking operation from a worker to keep the native event loop responsive.
///
/// # Errors
/// Rejects invalid printer names, copy counts, page ranges and rendering/spooler errors.
pub fn submit(doc: &RichDocument, page: &PageSettings, options: &PrintOptions) -> Result<()> {
    ensure!(
        !options.printer.trim().is_empty()
            && options.printer.len() <= 4096
            && !options.printer.chars().any(char::is_control),
        "Choose a valid system printer."
    );
    ensure!(
        (1..=999).contains(&options.copies),
        "Choose between 1 and 999 print copies."
    );
    let bytes = pdf(doc, page, false, options.page_range.as_deref())?;
    let job = craft_print::Job {
        printer: Some(options.printer.clone()),
        copies: options.copies,
        collate: true,
        duplex: Some(if options.duplex {
            craft_print::Duplex::LongEdge
        } else {
            craft_print::Duplex::Off
        }),
        grayscale: options.grayscale,
        scaling: craft_print::Scaling::ShrinkToFit,
        title: "ExEditor document".into(),
        ..Default::default()
    };
    craft_print::submit(&bytes, &job).context("Submitting the native print job")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_ranges_are_bounded_deduplicated_and_zero_based() {
        assert_eq!(parse_page_range("1-3,2,5", 5).unwrap(), vec![0, 1, 2, 4]);
        assert_eq!(parse_page_range("even", 5).unwrap(), vec![1, 3]);
        assert_eq!(parse_page_range("odd,5", 5).unwrap(), vec![0, 2, 4]);
        assert_eq!(parse_page_range("3-", 5).unwrap(), vec![2, 3, 4]);
        assert_eq!(parse_page_range("-2", 5).unwrap(), vec![0, 1]);
        for invalid in [
            "0",
            "6",
            "3-1",
            "1-2-3",
            "",
            "1,",
            "999999999999999999999999999",
        ] {
            assert!(parse_page_range(invalid, 5).is_err(), "{invalid}");
        }
        assert!(parse_page_range("even", 1).is_err());
    }

    #[test]
    fn page_setup_rejects_invalid_geometry_without_changing_the_snapshot() {
        let mut doc = RichDocument::from_text("retained text");
        let before = serde_json::to_value(&doc).unwrap();
        let invalid = PageSettings {
            margin_left: i32::MAX,
            ..Default::default()
        };
        assert!(apply_page_settings(&mut doc, &invalid).is_err());
        assert_eq!(serde_json::to_value(&doc).unwrap(), before);
        let valid = PageSettings {
            width_twips: 16840,
            height_twips: 11900,
            margin_left: 1200,
            margin_right: 1400,
            margin_top: 1600,
            margin_bottom: 1800,
        };
        apply_page_settings(&mut doc, &valid).unwrap();
        assert_eq!(doc.page.width_twips, 16840);
        assert_eq!(doc.page.margin_bottom, 1800);
        assert!(doc.html.contains("retained text"));
    }

    #[test]
    fn failed_export_never_replaces_existing_output() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("existing.pdf");
        std::fs::write(&path, b"original").unwrap();
        let invalid = PageSettings {
            width_twips: 0,
            ..Default::default()
        };
        assert!(export_pdf(&RichDocument::from_text("new text"), &path, &invalid).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
    }

    #[test]
    fn invalid_job_options_fail_before_any_printer_is_contacted() {
        let doc = RichDocument::from_text("text");
        for options in [
            PrintOptions::default(),
            PrintOptions {
                printer: "bad\nprinter".into(),
                ..Default::default()
            },
            PrintOptions {
                printer: "queue".into(),
                copies: 0,
                ..Default::default()
            },
            PrintOptions {
                printer: "queue".into(),
                copies: 1000,
                ..Default::default()
            },
        ] {
            assert!(submit(&doc, &PageSettings::default(), &options).is_err());
        }
    }
}
