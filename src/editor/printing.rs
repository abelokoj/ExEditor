//! Native PDF export and system printer submission share the document's page geometry.

use super::types::PageSettings;
use anyhow::{Context as _, Result, ensure};
use std::{io::Write as _, path::Path, sync::Arc};
use wordcraft_doc::{Block, Document, SectionProps};

/// One configured native system print queue.
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

/// Lists spooler queues without querying slow printer drivers.
pub fn printers() -> Result<Vec<Printer>> {
    ensure!(
        craft_print::backend().is_some(),
        "Native printing is unavailable on this platform."
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
    const MAX_TWIPS: i32 = 288_000; // PDF's conventional 200-inch physical page limit.
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

/// Sets physical page geometry before layout; twips are exactly one twentieth of a point.
/// Rejects invalid dimensions without changing any document properties.
pub fn apply_page_settings(doc: &mut Document, page: &PageSettings) -> Result<()> {
    validate_page_settings(page)?;
    let apply = |section: &mut SectionProps| {
        section.page_w = page.width_twips as f32 / 20.;
        section.page_h = page.height_twips as f32 / 20.;
        section.landscape = page.width_twips > page.height_twips;
        section.margin_left = page.margin_left as f32 / 20.;
        section.margin_right = page.margin_right as f32 / 20.;
        section.margin_top = page.margin_top as f32 / 20.;
        section.margin_bottom = page.margin_bottom as f32 / 20.;
    };
    apply(&mut doc.last_section);
    for block in &mut doc.body {
        if matches!(&**block, Block::Para(paragraph) if paragraph.section.is_some())
            && let Block::Para(paragraph) = Arc::make_mut(block)
            && let Some(section) = &mut paragraph.section
        {
            apply(section);
        }
    }
    Ok(())
}

fn output_document(doc: &Document, page: &PageSettings) -> Result<Document> {
    validate_page_settings(page)?;
    let section = &doc.last_section;
    let matches = [
        section.page_w,
        section.page_h,
        section.margin_left,
        section.margin_right,
        section.margin_top,
        section.margin_bottom,
    ]
    .into_iter()
    .map(|points| (points * 20.).round() as i32)
    .eq([
        page.width_twips,
        page.height_twips,
        page.margin_left,
        page.margin_right,
        page.margin_top,
        page.margin_bottom,
    ]);
    let mut snapshot = doc.clone();
    // Loaded documents may deliberately mix section sizes; Page Setup already updates all sections.
    if !matches {
        apply_page_settings(&mut snapshot, page)?;
    }
    Ok(snapshot)
}

fn parse_page_range(spec: &str, total: usize) -> Result<Vec<usize>> {
    ensure!(total > 0, "The document has no printable pages.");
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
    doc: &Document,
    page: &PageSettings,
    links: bool,
    page_range: Option<&str>,
) -> Result<Vec<u8>> {
    let snapshot = output_document(doc, page)?;
    let mut options = wordcraft_pdf::PdfOptions {
        links,
        ..Default::default()
    };
    if let Some(spec) = page_range.filter(|spec| !spec.trim().is_empty()) {
        let layout = wordcraft_layout::layout(
            &snapshot,
            &mut wordcraft_layout::LayoutCache::new(),
            &wordcraft_layout::LayoutOptions {
                hide_deleted: true,
                ..Default::default()
            },
        );
        options.pages = Some(parse_page_range(spec, layout.pages.len())?);
        wordcraft_pdf::export_layout(&snapshot, &layout, &options)
            .context("rendering the selected PDF pages")
    } else {
        wordcraft_pdf::export(&snapshot, &options).context("rendering the document as PDF")
    }
}

/// Writes the fully rendered PDF atomically, preserving an existing file on failure.
pub fn export_pdf(doc: &Document, path: &Path, page: &PageSettings) -> Result<()> {
    let bytes = pdf(doc, page, true, None)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut output = tempfile::NamedTempFile::new_in(parent).context("creating the PDF output")?;
    output.write_all(&bytes).context("writing the PDF")?;
    output.as_file().sync_all().context("saving the PDF")?;
    output
        .persist(path)
        .map_err(|error| error.error)
        .context("replacing the PDF output")?;
    Ok(())
}

/// Sends a PDF to the native spooler using the settings confirmed in the print form.
/// The caller runs this blocking operation on a worker, keeping the native event loop responsive.
pub fn submit(doc: &Document, page: &PageSettings, options: &PrintOptions) -> Result<()> {
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
    craft_print::submit(&bytes, &job).context("submitting the native print job")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_ranges_are_bounded_deduplicated_and_zero_based() {
        assert_eq!(parse_page_range("1-3,2,5", 5).unwrap(), vec![0, 1, 2, 4]);
        assert_eq!(parse_page_range("even", 5).unwrap(), vec![1, 3]);
        assert_eq!(parse_page_range("3-", 5).unwrap(), vec![2, 3, 4]);
        for invalid in ["0", "6", "3-1", "1-2-3", ""] {
            assert!(parse_page_range(invalid, 5).is_err(), "{invalid}");
        }
    }
    #[test]
    fn printing_preserves_mixed_sections_when_canonical_page_setup_matches() {
        let mut doc = Document::default();
        let first = SectionProps {
            page_w: 595.,
            page_h: 842.,
            margin_left: 40.,
            ..Default::default()
        };
        doc.body[0] = Arc::new(Block::Para(wordcraft_doc::Paragraph {
            section: Some(Box::new(first)),
            ..Default::default()
        }));
        let canonical = PageSettings {
            width_twips: 12240,
            height_twips: 15840,
            margin_left: 1440,
            margin_right: 1440,
            margin_top: 1440,
            margin_bottom: 1440,
        };
        assert_eq!(
            output_document(&doc, &canonical).expect("canonical page setup"),
            doc,
            "PDF export must retain imported section geometry"
        );
        let changed = PageSettings {
            width_twips: 16840,
            height_twips: 11900,
            ..canonical
        };
        let output = output_document(&doc, &changed).expect("changed page setup");
        assert!(
            output
                .sections()
                .iter()
                .all(|(_, section)| section.page_w == 842. && section.page_h == 595.)
        );
        assert_eq!(
            doc.body[0]
                .as_para()
                .and_then(|paragraph| paragraph.section.as_ref())
                .map(|section| section.page_w),
            Some(595.),
            "export must not change the live document"
        );
    }

    #[test]
    fn page_setup_reflows_every_section_with_twips_converted_once() {
        let mut doc = Document::default();
        let paragraph = wordcraft_doc::Paragraph {
            section: Some(Box::new(SectionProps::default())),
            ..Default::default()
        };
        doc.body.push(Arc::new(Block::Para(paragraph)));
        let page = PageSettings {
            width_twips: 16840,
            height_twips: 11900,
            margin_left: 1200,
            margin_right: 1400,
            margin_top: 1600,
            margin_bottom: 1800,
        };
        apply_page_settings(&mut doc, &page).expect("valid page setup");
        for (_, section) in doc.sections() {
            assert_eq!((section.page_w, section.page_h), (842., 595.));
            assert_eq!(
                (
                    section.margin_left,
                    section.margin_right,
                    section.margin_top,
                    section.margin_bottom
                ),
                (60., 70., 80., 90.)
            );
            assert!(section.landscape);
        }
        let before = doc.clone();
        let invalid = PageSettings {
            margin_left: i32::MAX,
            ..page
        };
        assert!(apply_page_settings(&mut doc, &invalid).is_err());
        assert_eq!(doc, before, "invalid geometry must not modify any section");
    }
}
