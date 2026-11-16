//! Shaped text and embedded media arranged into indivisible printable bands.

use super::markup::{Block, Style};
use anyhow::{Context as _, Result, ensure};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, LayoutGlyph, Metrics, Shaping, Weight, Wrap};
use std::{io::Cursor, sync::Arc};

#[derive(Clone, Debug)]
pub(super) enum Item {
    Text {
        text: Arc<str>,
        glyphs: Vec<LayoutGlyph>,
        styles: Arc<Vec<Style>>,
        baseline: f32,
    },
    Image {
        image: krilla::image::Image,
        width: f32,
        height: f32,
    },
    Line {
        end_x: f32,
        end_y: f32,
        color: [u8; 4],
        width: f32,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Placed {
    pub x: f32,
    pub y: f32,
    pub item: Item,
}

#[derive(Clone, Debug)]
pub(super) struct Band {
    pub height: f32,
    pub items: Vec<Placed>,
}

#[derive(Default)]
struct Budget {
    glyphs: usize,
    image_bytes: u64,
}

pub(super) fn typeset(
    blocks: &[Block],
    width: f32,
    height: f32,
    fonts: &mut FontSystem,
) -> Result<Vec<Band>> {
    blocks_layout(blocks, width, height, fonts, &mut Budget::default())
}

fn blocks_layout(
    blocks: &[Block],
    width: f32,
    height: f32,
    fonts: &mut FontSystem,
    budget: &mut Budget,
) -> Result<Vec<Band>> {
    let mut bands = Vec::new();
    for block in blocks {
        match block {
            Block::Paragraph { spans, style } => {
                let indent = style.indent.clamp(0., width * 0.8);
                let first_indent = style.first_indent.clamp(-indent, width * 0.8 - indent);
                let available = (width - indent - first_indent.max(0.)).max(0.01);
                let styles = Arc::new(if spans.is_empty() {
                    vec![style.clone()]
                } else {
                    spans.iter().map(|span| span.style.clone()).collect()
                });
                let mut buffer = Buffer::new(
                    fonts,
                    Metrics::new(style.size, style.size * style.line_height),
                );
                buffer.set_size(Some(available), None);
                buffer.set_wrap(Wrap::WordOrGlyph);
                if spans.is_empty() {
                    buffer.set_text("", &attributes(style, 0), Shaping::Advanced, style.align);
                } else {
                    buffer.set_rich_text(
                        spans
                            .iter()
                            .enumerate()
                            .map(|(i, span)| (span.text.as_str(), attributes(&span.style, i))),
                        &attributes(style, 0),
                        Shaping::Advanced,
                        style.align,
                    );
                }
                buffer.shape_until_scroll(fonts, false);
                let line_text: Vec<Arc<str>> = buffer
                    .lines
                    .iter()
                    .map(|line| Arc::from(line.text()))
                    .collect();
                let raised = styles.iter().map(|s| s.shift.max(0.)).fold(0., f32::max);
                let lowered = styles.iter().map(|s| (-s.shift).max(0.)).fold(0., f32::max);
                let before = bands.len();
                for (index, run) in buffer.layout_runs().enumerate() {
                    budget.glyphs += run.glyphs.len();
                    ensure!(
                        budget.glyphs <= 5_000_000,
                        "The document exceeds the print layout's glyph limit."
                    );
                    let x = indent + if index == 0 { first_indent } else { 0. };
                    bands.push(Band {
                        height: run.line_height + raised + lowered,
                        items: vec![Placed {
                            x,
                            y: 0.,
                            item: Item::Text {
                                text: line_text[run.line_i].clone(),
                                glyphs: run.glyphs.to_vec(),
                                styles: styles.clone(),
                                baseline: run.line_y - run.line_top + raised,
                            },
                        }],
                    });
                }
                if bands.len() == before {
                    bands.push(Band {
                        height: style.size * style.line_height,
                        items: vec![],
                    });
                }
                if let Some(last) = bands.last_mut() {
                    last.height += style.size * 0.35;
                }
            }
            Block::Image {
                bytes,
                width: requested_width,
                height: requested_height,
                align,
            } => {
                let mut reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
                    .with_guessed_format()
                    .context("Identifying a print image")?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(32768);
                limits.max_image_height = Some(32768);
                limits.max_alloc = Some(256 * 1024 * 1024);
                reader.limits(limits);
                let pixels = reader
                    .decode()
                    .context("Decoding a print image")?
                    .to_rgba8();
                ensure!(
                    pixels.width() > 0 && pixels.height() > 0,
                    "A print image has no pixels."
                );
                budget.image_bytes += pixels.as_raw().len() as u64;
                ensure!(
                    budget.image_bytes <= 256 * 1024 * 1024,
                    "The print images exceed the decoded resource limit."
                );
                let intrinsic_w = pixels.width() as f32 * 0.75;
                let intrinsic_h = pixels.height() as f32 * 0.75;
                let (w, h) = match (requested_width, requested_height) {
                    (Some(w), Some(h)) => (*w, *h),
                    (Some(w), None) => (*w, intrinsic_h * w / intrinsic_w),
                    (None, Some(h)) => (intrinsic_w * h / intrinsic_h, *h),
                    (None, None) => (intrinsic_w, intrinsic_h),
                };
                let scale = (width / w).min(height / h).min(1.);
                let (w, h) = (w * scale, h * scale);
                let x = match align {
                    Some(cosmic_text::Align::Right | cosmic_text::Align::End) => width - w,
                    Some(cosmic_text::Align::Center) => (width - w) / 2.,
                    _ => 0.,
                };
                let dimensions = pixels.dimensions();
                let image =
                    krilla::image::Image::from_rgba8(pixels.into_raw(), dimensions.0, dimensions.1);
                bands.push(Band {
                    height: h + 4.,
                    items: vec![Placed {
                        x,
                        y: 0.,
                        item: Item::Image {
                            image,
                            width: w,
                            height: h,
                        },
                    }],
                });
            }
            Block::Table(rows) => {
                // shortcut: equal-width columns and unmerged row spans; add a grid solver for precise Office table geometry.
                let columns = rows
                    .iter()
                    .map(|r| r.iter().map(|c| c.columns).sum::<usize>())
                    .max()
                    .unwrap_or(1)
                    .max(1);
                ensure!(columns <= 1000, "The table has too many print columns.");
                for row in rows {
                    let padding = 4_f32.min(width / columns as f32 / 8.);
                    let mut cells = Vec::new();
                    let mut x = 0.;
                    for cell in row {
                        let cell_width = width * cell.columns as f32 / columns as f32;
                        let cell_bands = blocks_layout(
                            &cell.blocks,
                            (cell_width - 2. * padding).max(0.01),
                            (height - 2. * padding).max(0.01),
                            fonts,
                            budget,
                        )?;
                        cells.push((x, cell_width, cell_bands));
                        x += cell_width;
                    }
                    let count = cells.iter().map(|(_, _, b)| b.len()).max().unwrap_or(1);
                    for line in 0..count {
                        let top = if line == 0 { padding } else { 0. };
                        let bottom = if line + 1 == count { padding } else { 0. };
                        let row_height = cells
                            .iter()
                            .filter_map(|(_, _, b)| b.get(line))
                            .map(|b| b.height)
                            .fold(0., f32::max)
                            + top
                            + bottom;
                        let mut items = Vec::new();
                        for (x, cell_width, cell_bands) in &cells {
                            if let Some(band) = cell_bands.get(line) {
                                items.extend(band.items.iter().cloned().map(|mut placed| {
                                    placed.x += x + padding;
                                    placed.y += top;
                                    placed
                                }));
                            }
                            let mut edge = |x1, y1, x2, y2| {
                                items.push(Placed {
                                    x: x1,
                                    y: y1,
                                    item: Item::Line {
                                        end_x: x2 - x1,
                                        end_y: y2 - y1,
                                        color: [100, 100, 100, 255],
                                        width: 0.5,
                                    },
                                })
                            };
                            edge(*x, 0., *x, row_height);
                            edge(x + cell_width, 0., x + cell_width, row_height);
                            if line == 0 {
                                edge(*x, 0., x + cell_width, 0.);
                            }
                            if line + 1 == count {
                                edge(*x, row_height, x + cell_width, row_height);
                            }
                        }
                        bands.push(Band {
                            height: row_height,
                            items,
                        });
                    }
                }
                if let Some(last) = bands.last_mut() {
                    last.height += 6.;
                }
            }
            Block::Rule => bands.push(Band {
                height: 12.,
                items: vec![Placed {
                    x: 0.,
                    y: 6.,
                    item: Item::Line {
                        end_x: width,
                        end_y: 0.,
                        color: [100, 100, 100, 255],
                        width: 0.75,
                    },
                }],
            }),
        }
        ensure!(
            bands.len() <= 500_000,
            "The document exceeds the print layout's line limit."
        );
    }
    Ok(bands)
}

fn attributes(style: &Style, metadata: usize) -> Attrs<'_> {
    let family = match style.family.to_ascii_lowercase().as_str() {
        "serif" => Family::Serif,
        "sans-serif" | "sans serif" => Family::SansSerif,
        "monospace" => Family::Monospace,
        _ => Family::Name(&style.family),
    };
    Attrs::new()
        .family(family)
        .weight(if style.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        })
        .style(if style.italic {
            cosmic_text::Style::Italic
        } else {
            cosmic_text::Style::Normal
        })
        .metrics(Metrics::new(style.size, style.size * style.line_height))
        .metadata(metadata)
}

pub(super) struct PageBand<'a> {
    pub band: &'a Band,
    pub y: f32,
    pub scale: f32,
}

pub(super) fn paginate(bands: &[Band], width: f32, height: f32) -> Result<Vec<Vec<PageBand<'_>>>> {
    let mut pages = vec![Vec::new()];
    let mut y = 0.;
    for band in bands {
        ensure!(
            band.height.is_finite() && band.height > 0.,
            "The print layout contains an invalid line height."
        );
        let extent = band
            .items
            .iter()
            .map(|placed| {
                placed.x
                    + match &placed.item {
                        Item::Text { glyphs, .. } => {
                            glyphs.iter().map(|g| g.x + g.w).fold(0., f32::max)
                        }
                        Item::Image { width, .. } => *width,
                        Item::Line { end_x, .. } => *end_x,
                    }
            })
            .fold(0., f32::max);
        let scale = (height / band.height).min(width / extent.max(0.01)).min(1.);
        let band_height = band.height * scale;
        if y + band_height > height + 0.001 && y > 0. {
            ensure!(
                pages.len() < 20_000,
                "The document exceeds the printable page limit."
            );
            pages.push(Vec::new());
            y = 0.;
        }
        pages
            .last_mut()
            .context("The print layout has no page")?
            .push(PageBand { band, y, scale });
        y += band_height;
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_fonts() -> FontSystem {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(include_bytes!("../../../assets/fonts/inter/Inter-Regular.ttf").to_vec());
        db.set_sans_serif_family("Inter");
        FontSystem::new_with_locale_and_db("en-US".into(), db)
    }

    #[test]
    fn long_unicode_paragraph_wraps_and_paginates_without_losing_glyphs() {
        let blocks =
            super::super::markup::parse(&format!("<p>{}</p>", "café Ελληνικά words ".repeat(200)))
                .unwrap();
        let bands = typeset(&blocks, 180., 100., &mut test_fonts()).unwrap();
        assert!(bands.len() > 20, "the narrow text area must wrap");
        let pages = paginate(&bands, 180., 100.).unwrap();
        assert!(pages.len() > 2, "the wrapped text must paginate");
        assert_eq!(pages.iter().map(Vec::len).sum::<usize>(), bands.len());
        assert!(
            pages
                .iter()
                .flatten()
                .all(|b| b.y >= 0. && b.y + b.band.height * b.scale <= 100.001)
        );
        assert!(
            bands
                .iter()
                .flat_map(|b| &b.items)
                .all(|item| match &item.item {
                    Item::Text { glyphs, .. } => glyphs.iter().all(|g| g.glyph_id != 0),
                    _ => true,
                })
        );
    }

    #[test]
    fn embedded_images_keep_aspect_ratio_and_fit_the_printable_area() {
        use base64::Engine as _;
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            20,
            10,
            image::Rgba([50, 100, 200, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
        let blocks = super::super::markup::parse(&format!(
            "<p>before</p><img src='data:image/png;base64,{encoded}' width='200'><p>after</p>"
        ))
        .unwrap();
        let bands = typeset(&blocks, 60., 80., &mut test_fonts()).unwrap();
        let image = bands
            .iter()
            .flat_map(|b| &b.items)
            .find_map(|i| match &i.item {
                Item::Image { width, height, .. } => Some((*width, *height)),
                _ => None,
            })
            .unwrap();
        assert_eq!(image, (60., 30.));
        assert_eq!(
            bands
                .iter()
                .flat_map(|b| &b.items)
                .filter(|i| matches!(&i.item, Item::Text { .. }))
                .count(),
            2
        );
    }
    #[test]
    fn tall_bands_fit_small_pages_and_tables_keep_cell_text() {
        let blocks = super::super::markup::parse(
            "<table><tr><td>left words</td><td><b>right words</b></td></tr></table>",
        )
        .unwrap();
        let bands = typeset(&blocks, 100., 20., &mut test_fonts()).unwrap();
        let pages = paginate(&bands, 100., 5.).unwrap();
        assert!(
            pages
                .iter()
                .flatten()
                .all(|b| b.band.height * b.scale <= 5.001)
        );
        let text = bands
            .iter()
            .flat_map(|b| &b.items)
            .filter_map(|i| match &i.item {
                Item::Text { text, .. } => Some(text.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("left words") && text.contains("right words"));
    }
}
