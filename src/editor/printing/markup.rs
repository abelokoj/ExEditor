//! A bounded print view of the native editor's HTML interchange snapshot.

use anyhow::{Context as _, Result, ensure};
use base64::Engine as _;
use cosmic_text::Align;
use scraper::{ElementRef, Html};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(super) struct Style {
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub shift: f32,
    pub preserve: bool,
    pub align: Option<Align>,
    pub indent: f32,
    pub first_indent: f32,
    pub line_height: f32,
    pub link: Option<String>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            family: "Inter".into(),
            size: 12.,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: [0, 0, 0, 255],
            background: None,
            shift: 0.,
            preserve: false,
            align: None,
            indent: 0.,
            first_indent: 0.,
            line_height: 1.3,
            link: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug)]
pub(super) struct Cell {
    pub blocks: Vec<Block>,
    pub columns: usize,
}

#[derive(Clone, Debug)]
pub(super) enum Block {
    Paragraph {
        spans: Vec<Span>,
        style: Style,
    },
    Image {
        bytes: Arc<Vec<u8>>,
        width: Option<f32>,
        height: Option<f32>,
        align: Option<Align>,
    },
    Table(Vec<Vec<Cell>>),
    Rule,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    paragraph: Style,
}

impl Builder {
    fn flush(&mut self, empty: bool) {
        while let Some(last) = self.spans.last_mut() {
            if !last.style.preserve {
                last.text.truncate(last.text.trim_end_matches(' ').len());
            }
            if !last.text.is_empty() {
                break;
            }
            self.spans.pop();
        }
        if empty || !self.spans.is_empty() {
            self.blocks.push(Block::Paragraph {
                spans: std::mem::take(&mut self.spans),
                style: self.paragraph.clone(),
            });
        }
    }

    fn text(&mut self, source: &str, style: &Style) {
        let mut text = String::new();
        let mut space = self
            .spans
            .last()
            .is_none_or(|span| span.text.ends_with([' ', '\n']));
        for ch in source.chars() {
            if !style.preserve && matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000c}') {
                if !space {
                    text.push(' ');
                }
                space = true;
            } else {
                text.push(ch);
                space = false;
            }
        }
        if !text.is_empty() {
            self.spans.push(Span {
                text,
                style: style.clone(),
            });
        }
    }

    fn children(&mut self, element: ElementRef<'_>, style: &Style) -> Result<()> {
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                self.text(text, style);
            } else if let Some(element) = ElementRef::wrap(child) {
                self.element(element, style)?;
            }
        }
        Ok(())
    }

    fn element(&mut self, element: ElementRef<'_>, inherited: &Style) -> Result<()> {
        let tag = element.value().name();
        let style = element_style(element, inherited);
        match tag {
            "br" => self.spans.push(Span {
                text: "\n".into(),
                style,
            }),
            "hr" => {
                self.flush(false);
                self.blocks.push(Block::Rule);
            }
            "img" => {
                self.flush(false);
                let source = element
                    .value()
                    .attr("src")
                    .context("An image has no embedded data")?;
                let (header, encoded) =
                    source.split_once(',').context("An image is not embedded")?;
                ensure!(
                    header.starts_with("data:image/") && header.ends_with(";base64"),
                    "Only embedded images can be printed."
                );
                ensure!(
                    encoded.len() <= 90 * 1024 * 1024,
                    "A print image exceeds its size limit."
                );
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .context("Decoding a print image")?;
                let dimension = |key| {
                    element
                        .value()
                        .attr(key)
                        .and_then(|s| s.parse::<f32>().ok())
                        .filter(|n| n.is_finite() && *n > 0.)
                        .map(|n| n * 0.75)
                };
                self.blocks.push(Block::Image {
                    bytes: Arc::new(bytes),
                    width: dimension("width"),
                    height: dimension("height"),
                    align: style.align,
                });
            }
            "table" => {
                self.flush(false);
                let mut rows = Vec::new();
                table_rows(element, &style, &mut rows)?;
                if !rows.is_empty() {
                    self.blocks.push(Block::Table(rows));
                }
            }
            "ul" | "ol" => {
                self.flush(false);
                let start = element
                    .value()
                    .attr("start")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1);
                let mut index = start;
                for child in element.child_elements() {
                    if child.value().name() != "li" {
                        self.element(child, &style)?;
                        continue;
                    }
                    let mut item_style = element_style(child, &style);
                    item_style.indent += 16.;
                    self.paragraph = item_style.clone();
                    self.text(
                        &if tag == "ol" {
                            format!("{index}. ")
                        } else {
                            "• ".into()
                        },
                        &item_style,
                    );
                    for node in child.children() {
                        if let Some(text) = node.value().as_text() {
                            self.text(text, &item_style);
                        } else if let Some(inner) = ElementRef::wrap(node) {
                            if matches!(inner.value().name(), "p" | "div") && !self.spans.is_empty()
                            {
                                self.children(inner, &element_style(inner, &item_style))?;
                                self.flush(false);
                            } else {
                                self.element(inner, &item_style)?;
                            }
                        }
                    }
                    self.flush(false);
                    index = index.saturating_add(1);
                }
            }
            "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "pre" | "blockquote" | "li" => {
                self.flush(false);
                self.paragraph = style.clone();
                let before = self.blocks.len();
                self.children(element, &style)?;
                self.flush(self.blocks.len() == before);
                self.paragraph = inherited.clone();
            }
            _ => self.children(element, &style)?,
        }
        Ok(())
    }
}

fn table_rows(element: ElementRef<'_>, style: &Style, rows: &mut Vec<Vec<Cell>>) -> Result<()> {
    for child in element.child_elements() {
        match child.value().name() {
            "thead" | "tbody" | "tfoot" => table_rows(child, style, rows)?,
            "tr" => {
                let mut row = Vec::new();
                for cell in child
                    .child_elements()
                    .filter(|e| matches!(e.value().name(), "td" | "th"))
                {
                    let cell_style = element_style(cell, style);
                    let mut builder = Builder {
                        paragraph: cell_style.clone(),
                        ..Default::default()
                    };
                    builder.children(cell, &cell_style)?;
                    builder.flush(builder.blocks.is_empty());
                    row.push(Cell {
                        blocks: builder.blocks,
                        columns: cell
                            .value()
                            .attr("colspan")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1)
                            .clamp(1, 1000),
                    });
                }
                ensure!(
                    rows.len() < 100_000 && row.len() <= 1000,
                    "The print table exceeds its row or column limit."
                );
                if !row.is_empty() {
                    rows.push(row);
                }
            }
            "caption" => {}
            _ => {}
        }
    }
    Ok(())
}

fn element_style(element: ElementRef<'_>, inherited: &Style) -> Style {
    let mut style = inherited.clone();
    match element.value().name() {
        "b" | "strong" | "th" => style.bold = true,
        "i" | "em" => style.italic = true,
        "u" => style.underline = true,
        "s" | "strike" | "del" => style.strike = true,
        "code" => style.family = crate::fonts::default_plain_font().into(),
        "pre" => {
            style.family = crate::fonts::default_plain_font().into();
            style.preserve = true;
        }
        "blockquote" => style.indent += 18.,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            style.size = match element.value().name() {
                "h1" => 24.,
                "h2" => 20.,
                "h3" => 17.,
                "h4" => 14.,
                _ => 12.,
            };
            style.bold = true;
        }
        "sub" => {
            style.shift -= style.size * 0.2;
            style.size *= 0.75;
        }
        "sup" => {
            style.shift += style.size * 0.35;
            style.size *= 0.75;
        }
        "a" => {
            style.link = element.value().attr("href").map(str::to_owned);
        }
        _ => {}
    }
    if element.value().attr("dir") == Some("rtl") {
        style.align = Some(Align::Right);
    }
    for declaration in element.value().attr("style").unwrap_or("").split(';') {
        let Some((key, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                style.family = value
                    .split(',')
                    .next()
                    .unwrap_or("Inter")
                    .trim()
                    .trim_matches(['\'', '"'])
                    .into();
            }
            "font-size" => {
                if let Some(size) = points(value, style.size) {
                    style.size = size.clamp(1., 512.);
                }
            }
            "font-weight" => {
                style.bold = matches!(value, "bold" | "bolder")
                    || value.parse::<u16>().is_ok_and(|n| n >= 600)
            }
            "font-style" => style.italic = matches!(value, "italic" | "oblique"),
            "text-decoration" => {
                style.underline = value.contains("underline");
                style.strike = value.contains("line-through");
            }
            "color" => {
                if let Ok(color) = value.parse::<csscolorparser::Color>() {
                    style.color = color.to_rgba8();
                }
            }
            "background-color" => {
                style.background = value
                    .parse::<csscolorparser::Color>()
                    .ok()
                    .map(|c| c.to_rgba8())
            }
            "text-align" => {
                style.align = match value {
                    "left" => Some(Align::Left),
                    "center" => Some(Align::Center),
                    "right" => Some(Align::Right),
                    "justify" => Some(Align::Justified),
                    _ => None,
                }
            }
            "margin-left" => {
                if let Some(indent) = points(value, style.size) {
                    style.indent = indent.max(0.);
                }
            }
            "text-indent" => {
                if let Some(indent) = points(value, style.size) {
                    style.first_indent = indent;
                }
            }
            "line-height" => {
                let ratio = value
                    .parse::<f32>()
                    .ok()
                    .or_else(|| points(value, style.size).map(|n| n / style.size));
                if let Some(ratio) = ratio.filter(|n| n.is_finite() && *n > 0.) {
                    style.line_height = ratio.clamp(1., 10.);
                }
            }
            "white-space" => style.preserve = matches!(value, "pre" | "pre-wrap" | "break-spaces"),
            "direction" => {
                if value == "rtl" {
                    style.align = Some(Align::Right);
                }
            }
            "vertical-align" => match value {
                "super" => {
                    style.shift = style.size * 0.35;
                    style.size *= 0.75;
                }
                "sub" => {
                    style.shift = -style.size * 0.2;
                    style.size *= 0.75;
                }
                _ => {}
            },
            _ => {}
        }
    }
    style
}

fn points(value: &str, em: f32) -> Option<f32> {
    let value = value.trim().to_ascii_lowercase();
    let (number, factor) = if let Some(n) = value.strip_suffix("pt") {
        (n, 1.)
    } else if let Some(n) = value.strip_suffix("px") {
        (n, 0.75)
    } else if let Some(n) = value.strip_suffix("in") {
        (n, 72.)
    } else if let Some(n) = value.strip_suffix("cm") {
        (n, 72. / 2.54)
    } else if let Some(n) = value.strip_suffix("mm") {
        (n, 72. / 25.4)
    } else if let Some(n) = value.strip_suffix("em") {
        (n, em)
    } else if let Some(n) = value.strip_suffix('%') {
        (n, em / 100.)
    } else {
        (value.as_str(), 0.75)
    };
    number
        .parse::<f32>()
        .ok()
        .map(|n| n * factor)
        .filter(|n| n.is_finite())
}

pub(super) fn parse(source: &str) -> Result<Vec<Block>> {
    let safe = crate::formats::safe_native_html(source)?;
    let html = Html::parse_fragment(&safe);
    let style = Style::default();
    let mut builder = Builder {
        paragraph: style.clone(),
        ..Default::default()
    };
    builder.children(html.root_element(), &style)?;
    builder.flush(builder.blocks.is_empty());
    Ok(builder.blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_markup_keeps_unicode_styles_tables_and_blank_lines() {
        let blocks = parse("<p>Hello <b style='color:#ff0000'>café Ελληνικά 😀</b><br>next</p><p></p><table><tr><th>A</th><td colspan='2'>B</td></tr></table>").unwrap();
        let Block::Paragraph { spans, .. } = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(
            spans.iter().map(|s| s.text.as_str()).collect::<String>(),
            "Hello café Ελληνικά 😀\nnext"
        );
        assert_eq!(spans[0].style.size, 12.);
        assert!(spans[1].style.bold);
        assert_eq!(spans[1].style.color, [255, 0, 0, 255]);
        assert!(matches!(&blocks[1], Block::Paragraph { spans, .. } if spans.is_empty()));
        assert!(matches!(&blocks[2], Block::Table(rows) if rows[0][1].columns == 2));
    }

    #[test]
    fn list_markers_share_a_line_with_native_paragraph_children() {
        let blocks =
            parse("<ol start='3'><li><p>first</p></li><li><p>second</p></li></ol>").unwrap();
        let text = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph { spans, .. } => {
                    Some(spans.iter().map(|s| s.text.as_str()).collect::<String>())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text, ["3. first", "4. second"]);
    }
    #[test]
    fn collapses_html_spaces_but_preserves_preformatted_text() {
        let blocks = parse("<p> a   <i> b </i> c </p><pre> a\t b\n c </pre>").unwrap();
        let texts = blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph { spans, .. } => {
                    spans.iter().map(|s| s.text.as_str()).collect::<String>()
                }
                _ => String::new(),
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, ["a b c", " a\t b\n c "]);
    }
}
