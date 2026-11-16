//! PDF drawing uses the same positioned glyphs for export and native printer jobs.

use super::{
    layout::{Item, PageBand},
    markup::Style,
};
use crate::editor::PageSettings;
use anyhow::{Context as _, Result, ensure};
use cosmic_text::{FontSystem, LayoutGlyph, fontdb};
use krilla::{
    Document,
    action::LinkAction,
    annotation::{Annotation, LinkAnnotation, Target},
    color::rgb,
    geom::{PathBuilder, Point, Rect, Size, Transform},
    num::NormalizedF32,
    paint::{Fill, Stroke},
    surface::Surface,
    text::{Font, GlyphId, KrillaGlyph, Tag},
};
use std::collections::{HashMap, hash_map::Entry};

type FontCache = HashMap<(fontdb::ID, u16), Font>;

struct TextRun<'a> {
    glyphs: &'a [LayoutGlyph],
    text: &'a str,
    styles: &'a [Style],
    origin: Point,
}

pub(super) fn draw(
    pages: &[Vec<PageBand<'_>>],
    selected: &[usize],
    settings: &PageSettings,
    fonts: &FontSystem,
    links: bool,
) -> Result<Vec<u8>> {
    let mut document = Document::new();
    let mut font_cache = FontCache::new();
    let width = settings.width_twips as f32 / 20.;
    let height = settings.height_twips as f32 / 20.;
    let left = settings.margin_left as f32 / 20.;
    let top = settings.margin_top as f32 / 20.;
    let content_width = width - left - settings.margin_right as f32 / 20.;
    let content_height = height - top - settings.margin_bottom as f32 / 20.;
    for &index in selected {
        let mut page = document.start_page_with(
            krilla::page::PageSettings::from_wh(width, height)
                .context("The PDF page dimensions are invalid")?,
        );
        let mut annotations = Vec::new();
        let mut surface = page.surface();
        let mut clip = PathBuilder::new();
        clip.push_rect(
            Rect::from_xywh(left, top, content_width, content_height)
                .context("The printable area is invalid")?,
        );
        let clip = clip.finish().context("The printable area is empty")?;
        surface.push_clip_path(&clip, &Default::default());
        let drawing = (|| -> Result<()> {
            for placed_band in &pages[index] {
                let scale = placed_band.scale;
                surface.push_transform(&Transform::from_row(
                    scale,
                    0.,
                    0.,
                    scale,
                    left,
                    top + placed_band.y,
                ));
                let band_result = (|| -> Result<()> {
                    for placed in &placed_band.band.items {
                        match &placed.item {
                            Item::Text {
                                text,
                                glyphs,
                                styles,
                                baseline,
                            } => {
                                let run = TextRun {
                                    glyphs,
                                    text,
                                    styles,
                                    origin: Point::from_xy(placed.x, placed.y + baseline),
                                };
                                text_line(&mut surface, &run, fonts, &mut font_cache)?;
                                if links {
                                    add_links(
                                        &run,
                                        Point::from_xy(left, top + placed_band.y),
                                        scale,
                                        &mut annotations,
                                    );
                                }
                            }
                            Item::Image {
                                image,
                                width,
                                height,
                            } => {
                                let size = Size::from_wh(*width, *height)
                                    .context("The print image dimensions are invalid")?;
                                surface
                                    .push_transform(&Transform::from_translate(placed.x, placed.y));
                                surface.draw_image(image.clone(), size);
                                surface.pop();
                            }
                            Item::Line {
                                end_x,
                                end_y,
                                color,
                                width,
                            } => {
                                line(
                                    &mut surface,
                                    (placed.x, placed.y),
                                    (placed.x + end_x, placed.y + end_y),
                                    *color,
                                    *width,
                                );
                            }
                        }
                    }
                    Ok(())
                })();
                surface.pop();
                band_result?;
            }
            Ok(())
        })();
        surface.pop();
        surface.finish();
        drawing?;
        for annotation in annotations {
            page.add_annotation(annotation);
        }
        page.finish();
    }
    document.finish().context("Finishing the PDF document")
}

fn text_line(
    surface: &mut Surface<'_>,
    run: &TextRun<'_>,
    fonts: &FontSystem,
    font_cache: &mut FontCache,
) -> Result<()> {
    let (glyphs, text, styles, x, baseline) =
        (run.glyphs, run.text, run.styles, run.origin.x, run.origin.y);
    for glyph in glyphs {
        let style = &styles[glyph.metadata];
        if let Some(background) = style.background {
            rectangle(
                surface,
                (x + glyph.x, baseline - glyph.font_size - style.shift),
                (glyph.w, glyph.font_size * 1.2),
                background,
            );
        }
    }
    let mut offset = 0;
    while offset < glyphs.len() {
        let first = &glyphs[offset];
        let mut end = offset + 1;
        while end < glyphs.len() {
            let next = &glyphs[end];
            if next.font_id != first.font_id
                || next.font_weight != first.font_weight
                || next.font_size != first.font_size
                || next.metadata != first.metadata
            {
                break;
            }
            end += 1;
        }
        let style = &styles[first.metadata];
        let group = &glyphs[offset..end];
        let mut pen = first.x;
        let mut shaped = Vec::with_capacity(group.len());
        for glyph in group {
            let cluster = text
                .get(glyph.start..glyph.end)
                .context("The print layout has an invalid text cluster")?;
            ensure!(
                glyph.glyph_id != 0
                    || cluster.chars().all(|c| c.is_whitespace()
                        || c.is_control()
                        || matches!(c, '\u{200c}' | '\u{200d}' | '\u{fe0e}' | '\u{fe0f}')),
                "No installed or accessory font contains {cluster:?}. Add a font with these characters in Settings > Accessories or install a suitable system font."
            );
            shaped.push(KrillaGlyph {
                glyph_id: GlyphId::new(u32::from(glyph.glyph_id)),
                text_range: glyph.start..glyph.end,
                x_advance: glyph.w / first.font_size,
                x_offset: (glyph.x - pen) / first.font_size + glyph.x_offset,
                y_offset: glyph.y_offset + (style.shift - glyph.y) / first.font_size,
                y_advance: 0.,
                location: None,
            });
            pen += glyph.w;
        }
        let font = match font_cache.entry((first.font_id, first.font_weight.0)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let font = fonts
                    .db()
                    .with_face_data(first.font_id, |data, index| {
                        Font::new_variable(
                            data.to_vec().into(),
                            index,
                            &[(Tag::new(b"wght"), f32::from(first.font_weight.0))],
                        )
                    })
                    .flatten()
                    .context("The selected output font cannot be embedded in a PDF")?;
                entry.insert(font)
            }
        };
        surface.set_stroke(None);
        surface.set_fill(Some(fill(style.color)));
        surface.draw_glyphs(
            Point::from_xy(x + first.x, baseline),
            &shaped,
            font.clone(),
            text,
            first.font_size,
            false,
        );
        offset = end;
    }
    for glyph in glyphs {
        let style = &styles[glyph.metadata];
        let thickness = (glyph.font_size / 16.).max(0.5);
        if style.underline {
            line(
                surface,
                (x + glyph.x, baseline + glyph.font_size * 0.1 - style.shift),
                (
                    x + glyph.x + glyph.w,
                    baseline + glyph.font_size * 0.1 - style.shift,
                ),
                style.color,
                thickness,
            );
        }
        if style.strike {
            line(
                surface,
                (x + glyph.x, baseline - glyph.font_size * 0.3 - style.shift),
                (
                    x + glyph.x + glyph.w,
                    baseline - glyph.font_size * 0.3 - style.shift,
                ),
                style.color,
                thickness,
            );
        }
    }
    Ok(())
}

fn fill(color: [u8; 4]) -> Fill {
    Fill {
        paint: rgb::Color::new(color[0], color[1], color[2]).into(),
        opacity: NormalizedF32::new(f32::from(color[3]) / 255.).unwrap_or(NormalizedF32::ONE),
        rule: Default::default(),
    }
}

fn rectangle(
    surface: &mut Surface<'_>,
    (x, y): (f32, f32),
    (width, height): (f32, f32),
    color: [u8; 4],
) {
    let Some(rect) = Rect::from_xywh(x, y, width, height) else {
        return;
    };
    let mut path = PathBuilder::new();
    path.push_rect(rect);
    if let Some(path) = path.finish() {
        surface.set_stroke(None);
        surface.set_fill(Some(fill(color)));
        surface.draw_path(&path);
    }
}

fn line(
    surface: &mut Surface<'_>,
    (x1, y1): (f32, f32),
    (x2, y2): (f32, f32),
    color: [u8; 4],
    width: f32,
) {
    let mut path = PathBuilder::new();
    path.move_to(x1, y1);
    path.line_to(x2, y2);
    if let Some(path) = path.finish() {
        surface.set_fill(None);
        surface.set_stroke(Some(Stroke {
            paint: rgb::Color::new(color[0], color[1], color[2]).into(),
            width,
            opacity: NormalizedF32::new(f32::from(color[3]) / 255.).unwrap_or(NormalizedF32::ONE),
            ..Default::default()
        }));
        surface.draw_path(&path);
    }
}

fn add_links(run: &TextRun<'_>, origin: Point, scale: f32, annotations: &mut Vec<Annotation>) {
    let (glyphs, styles, x, baseline, left, top) = (
        run.glyphs,
        run.styles,
        run.origin.x,
        run.origin.y,
        origin.x,
        origin.y,
    );
    let mut offset = 0;
    while offset < glyphs.len() {
        let first = &glyphs[offset];
        let style = &styles[first.metadata];
        let mut end = offset + 1;
        while end < glyphs.len() && glyphs[end].metadata == first.metadata {
            end += 1;
        }
        if let Some(link) = &style.link
            && !link.starts_with('#')
        {
            let group = &glyphs[offset..end];
            let start_x = group.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
            let end_x = group.iter().map(|g| g.x + g.w).fold(0., f32::max);
            let size = group.iter().map(|g| g.font_size).fold(0., f32::max);
            if let Some(rect) = Rect::from_xywh(
                left + (x + start_x) * scale,
                top + (baseline - size - style.shift) * scale,
                (end_x - start_x) * scale,
                size * 1.2 * scale,
            ) {
                annotations.push(Annotation::new_link(
                    LinkAnnotation::new(rect, Target::Action(LinkAction::new(link.clone()).into())),
                    Some(link.clone()),
                ));
            }
        }
        offset = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_standalone_pdf_with_embedded_unicode_text_and_color() {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../../../assets/fonts/inter/Inter-Regular.ttf").to_vec());
        db.set_sans_serif_family("Inter");
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let blocks = super::super::markup::parse(
            "<p style='color:#204080'>café Ελληνικά <a href='https://example.com'>link</a></p>",
        )
        .unwrap();
        let bands = super::super::layout::typeset(&blocks, 300., 500., &mut fonts).unwrap();
        let pages = super::super::layout::paginate(&bands, 300., 500.).unwrap();
        let pdf = draw(&pages, &[0], &PageSettings::default(), &fonts, true).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.7"));
        assert!(
            pdf.windows(5).rev().take(8).any(|tail| tail == b"%%EOF"),
            "the PDF must finish its cross-reference table"
        );
        assert!(
            pdf.len() > 1000,
            "the standalone PDF must include font data"
        );
        assert!(
            String::from_utf8_lossy(&pdf).contains("https://example.com"),
            "the PDF must retain safe hyperlinks"
        );
    }
    #[test]
    fn missing_output_glyph_returns_an_error_with_balanced_pdf_state() {
        let mut db = fontdb::Database::new();
        db.load_font_data(include_bytes!("../../../assets/fonts/inter/Inter-Regular.ttf").to_vec());
        let mut fonts = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let blocks = super::super::markup::parse("<p>A</p>").unwrap();
        let mut bands = super::super::layout::typeset(&blocks, 300., 500., &mut fonts).unwrap();
        let Item::Text { glyphs, .. } = &mut bands[0].items[0].item else {
            panic!("expected text");
        };
        glyphs[0].glyph_id = 0;
        let pages = super::super::layout::paginate(&bands, 300., 500.).unwrap();
        assert!(draw(&pages, &[0], &PageSettings::default(), &fonts, false).is_err());
    }
}
