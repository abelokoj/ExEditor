//! Internal, offline conversion between RichEdit RTF and common document packages.
//!
//! TXT and RTF are saved directly by `document`; package conversions reject unsupported
//! embedded objects rather than discarding them or invoking another application.

use crate::document::DocumentFormat;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use encoding_rs::WINDOWS_1252;
use roxmltree::{Document as XmlDocument, Node};
use scraper::{ElementRef, Html};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_PART: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
struct Style {
    font: String,
    size: f32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    foreground: u32,
    background: Option<u32>,
    position: i8,
    link: Option<String>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            font: "Helvetica".into(),
            size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            foreground: 0,
            background: None,
            position: 0,
            link: None,
        }
    }
}
#[derive(Debug, Clone)]
struct Picture {
    bytes: Vec<u8>,
    mime: String,
    width: i32,
    height: i32,
}
#[derive(Debug, Clone)]
struct Run {
    text: String,
    style: Style,
    picture: Option<Picture>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum List {
    #[default]
    None,
    Bullet,
    Number,
}
#[derive(Debug, Clone, Default)]
struct Paragraph {
    runs: Vec<Run>,
    alignment: Alignment,
    list: List,
    left: i32,
    right: i32,
    first: i32,
    before: i32,
    after: i32,
    line: i32,
}
#[derive(Debug, Clone)]
enum Block {
    Paragraph(Paragraph),
    Table(Vec<Vec<Vec<Paragraph>>>),
}
#[derive(Debug)]
struct Document {
    blocks: Vec<Block>,
    width: i32,
    height: i32,
    margins: [i32; 4],
}
impl Default for Document {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            width: 12240,
            height: 15840,
            margins: [1440; 4],
        }
    }
}

pub fn import(path: &Path, format: DocumentFormat) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("Reading {}", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_PART * 4,
        "The document exceeds the conversion safety limit."
    );
    let document = match format {
        DocumentFormat::Html => read_html(&bytes, path.parent().unwrap_or_else(|| Path::new(".")))?,
        DocumentFormat::Docx => read_docx(&bytes)?,
        DocumentFormat::Odt => read_odt(&bytes)?,
        DocumentFormat::Doc => bail!(
            "Legacy binary Word (.doc) documents are not supported by the internal converter. Save a .docx or .rtf copy in the original application; your original is unchanged."
        ),
        _ => bail!("This format is handled directly by the document editor"),
    };
    write_rtf(&document)
}

/// Renders Markdown (CommonMark with tables, task lists, strikethrough, and
/// footnotes) as a formatted rich-text document for reading.
pub fn markdown_to_rtf(source: &str, directory: &Path) -> Result<Vec<u8>> {
    use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    // Remote images cannot be shown offline, so their alt text stands in.
    let mut remote = Vec::new();
    let events = Parser::new_ext(source, options).filter_map(|event| match event {
        Event::Start(Tag::Image { ref dest_url, .. }) => {
            let is_remote = dest_url.contains(':') || dest_url.starts_with('/');
            remote.push(is_remote);
            (!is_remote).then_some(event)
        }
        Event::End(TagEnd::Image) => (!remote.pop().unwrap_or(false)).then_some(event),
        Event::TaskListMarker(done) => Some(Event::Text(CowStr::Borrowed(if done {
            "\u{2611} "
        } else {
            "\u{2610} "
        }))),
        other => Some(other),
    });
    let mut page = String::from("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>");
    html::push_html(&mut page, events);
    page.push_str("</body></html>");
    let mut document = parse_html(page.as_bytes(), directory, HtmlLinks::Preview)?;
    // Read as a document: the app's text font and a little room between blocks.
    let mut paragraphs: Vec<&mut Paragraph> = Vec::new();
    for block in &mut document.blocks {
        match block {
            Block::Paragraph(paragraph) => paragraphs.push(paragraph),
            Block::Table(rows) => paragraphs.extend(rows.iter_mut().flatten().flatten()),
        }
    }
    for paragraph in paragraphs {
        if paragraph.list == List::None {
            paragraph.after = 120;
        }
        for run in &mut paragraph.runs {
            if run.style.font == Style::default().font {
                run.style.font = crate::fonts::RICH_FONT.into();
            }
        }
    }
    write_rtf(&document)
}

pub fn export(rtf: &[u8], format: DocumentFormat) -> Result<Vec<u8>> {
    let document = read_rtf(rtf)
        .context("Converting the rich-text document; the original file has not been modified")?;
    match format {
        DocumentFormat::Html => write_html(&document).map(String::into_bytes),
        DocumentFormat::Docx => write_docx(&document),
        DocumentFormat::Odt => write_odt(&document),
        DocumentFormat::Doc => bail!(
            "Legacy binary Word (.doc) export is not supported. Choose Word (.docx) or Rich Text (.rtf); no file has been replaced."
        ),
        _ => bail!("This format is handled directly by the document editor"),
    }
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn value<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value())
}
fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
}
fn number(node: Node<'_, '_>, name: &str, default: i32) -> i32 {
    value(node, name)
        .and_then(|text| text.parse().ok())
        .unwrap_or(default)
}
fn on(node: Node<'_, '_>) -> bool {
    !matches!(value(node, "val"), Some("0" | "false" | "off" | "none"))
}
fn color(text: &str) -> Option<u32> {
    let text = text.trim().trim_start_matches('#');
    if text.len() == 6 {
        u32::from_str_radix(text, 16).ok()
    } else if text.len() == 3 {
        u32::from_str_radix(
            &text
                .chars()
                .flat_map(|character| [character, character])
                .collect::<String>(),
            16,
        )
        .ok()
    } else {
        match text.to_ascii_lowercase().as_str() {
            "black" => Some(0),
            "white" => Some(0xffffff),
            "red" => Some(0xff0000),
            "blue" => Some(0x0000ff),
            "green" => Some(0x008000),
            _ => None,
        }
    }
}
fn push_text(paragraph: &mut Paragraph, text: &str, style: &Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = paragraph.runs.last_mut()
        && last.picture.is_none()
        && last.style == *style
    {
        last.text.push_str(text);
        return;
    }
    paragraph.runs.push(Run {
        text: text.into(),
        style: style.clone(),
        picture: None,
    });
}
fn safe_link(link: &str) -> Result<String> {
    let allowed = link.starts_with('#')
        || link.split_once(':').is_some_and(|(scheme, rest)| {
            ((scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http"))
                && rest.starts_with("//"))
                || scheme.eq_ignore_ascii_case("mailto")
                || scheme.eq_ignore_ascii_case("tel")
        });
    ensure!(
        allowed
            && !link
                .chars()
                .any(|character| character.is_control() || character == '"'),
        "The document contains a link with an unsupported or unsafe URL scheme."
    );
    Ok(link.into())
}

#[derive(Debug, Clone)]
enum Token {
    Word(String, Option<i32>),
    Text(Vec<u8>),
    Byte(u8),
    Group(Vec<Token>),
    Binary(Vec<u8>),
}
fn tokens(bytes: &[u8], cursor: &mut usize, depth: usize) -> Result<Vec<Token>> {
    ensure!(
        depth <= 128,
        "The RTF document contains too many nested groups."
    );
    let mut result = Vec::new();
    while *cursor < bytes.len() {
        match bytes[*cursor] {
            b'{' => {
                *cursor += 1;
                result.push(Token::Group(tokens(bytes, cursor, depth + 1)?));
            }
            b'}' => {
                ensure!(depth > 0, "The RTF groups are unbalanced.");
                *cursor += 1;
                return Ok(result);
            }
            b'\r' | b'\n' => *cursor += 1,
            b'\\' => {
                *cursor += 1;
                let first = *bytes.get(*cursor).context("Incomplete RTF escape")?;
                if first == b'\'' {
                    let digits = bytes
                        .get(*cursor + 1..*cursor + 3)
                        .context("Incomplete RTF hexadecimal escape")?;
                    result.push(Token::Byte(u8::from_str_radix(
                        std::str::from_utf8(digits)?,
                        16,
                    )?));
                    *cursor += 3;
                } else if !first.is_ascii_alphabetic() {
                    *cursor += 1;
                    if matches!(first, b'\\' | b'{' | b'}') {
                        result.push(Token::Byte(first));
                    } else {
                        result.push(Token::Word(char::from(first).to_string(), None));
                    }
                } else {
                    let start = *cursor;
                    while bytes.get(*cursor).is_some_and(u8::is_ascii_alphabetic) {
                        *cursor += 1;
                    }
                    let word = std::str::from_utf8(&bytes[start..*cursor])?.to_owned();
                    let number_start = *cursor;
                    if bytes.get(*cursor) == Some(&b'-') {
                        *cursor += 1;
                    }
                    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
                        *cursor += 1;
                    }
                    let argument = if *cursor > number_start {
                        Some(std::str::from_utf8(&bytes[number_start..*cursor])?.parse::<i32>()?)
                    } else {
                        None
                    };
                    if bytes.get(*cursor) == Some(&b' ') {
                        *cursor += 1;
                    }
                    if word == "bin" {
                        let length =
                            usize::try_from(argument.context("Missing RTF binary length")?)?;
                        let end = (*cursor)
                            .checked_add(length)
                            .context("RTF binary length overflow")?;
                        result.push(Token::Binary(
                            bytes
                                .get(*cursor..end)
                                .context("Incomplete RTF binary data")?
                                .to_vec(),
                        ));
                        *cursor = end;
                    } else {
                        result.push(Token::Word(word, argument));
                    }
                }
            }
            _ => {
                let start = *cursor;
                while bytes
                    .get(*cursor)
                    .is_some_and(|byte| !matches!(byte, b'{' | b'}' | b'\\' | b'\r' | b'\n'))
                {
                    *cursor += 1;
                }
                result.push(Token::Text(bytes[start..*cursor].to_vec()));
            }
        }
    }
    ensure!(depth == 0, "The RTF document has an unclosed group.");
    Ok(result)
}
fn destination(group: &[Token]) -> Option<&str> {
    group.iter().find_map(|token| match token {
        Token::Word(word, _) if word != "*" => Some(word.as_str()),
        _ => None,
    })
}
fn token_text(group: &[Token]) -> Result<String> {
    let mut result = String::new();
    for token in group {
        match token {
            Token::Text(bytes) => {
                result.push_str(&WINDOWS_1252.decode_without_bom_handling(bytes).0)
            }
            Token::Byte(byte) => {
                result.push_str(&WINDOWS_1252.decode_without_bom_handling(&[*byte]).0)
            }
            Token::Group(nested) => result.push_str(&token_text(nested)?),
            _ => {}
        }
    }
    Ok(result)
}
fn words(group: &[Token], name: &str) -> Option<i32> {
    group.iter().find_map(|token| match token {
        Token::Word(word, argument) if word == name => *argument,
        _ => None,
    })
}

#[derive(Clone, Default)]
struct RtfState {
    style: Style,
    paragraph: Paragraph,
    unicode_skip: usize,
}
struct RtfReader {
    document: Document,
    current: Paragraph,
    row: Vec<Vec<Paragraph>>,
    cell: Vec<Paragraph>,
    table: Vec<Vec<Vec<Paragraph>>>,
    row_active: bool,
    fonts: BTreeMap<i32, String>,
    colors: Vec<Option<u32>>,
    skip: usize,
    high_surrogate: Option<u16>,
}
impl RtfReader {
    fn new() -> Self {
        Self {
            document: Document::default(),
            current: Paragraph::default(),
            row: Vec::new(),
            cell: Vec::new(),
            table: Vec::new(),
            row_active: false,
            fonts: BTreeMap::new(),
            colors: vec![None],
            skip: 0,
            high_surrogate: None,
        }
    }
    fn finish_table(&mut self) {
        if !self.table.is_empty() {
            self.document
                .blocks
                .push(Block::Table(std::mem::take(&mut self.table)));
        }
    }
    fn paragraph(&mut self, force: bool) {
        if !force && self.current.runs.is_empty() {
            return;
        }
        let next = Paragraph {
            runs: Vec::new(),
            ..self.current.clone()
        };
        let paragraph = std::mem::replace(&mut self.current, next);
        if self.row_active {
            self.cell.push(paragraph);
        } else {
            self.finish_table();
            self.document.blocks.push(Block::Paragraph(paragraph));
        }
    }
    fn text(&mut self, text: &str, state: &RtfState) {
        push_text(&mut self.current, text, &state.style);
    }
    fn code_unit(&mut self, word: u16, state: &RtfState) -> Result<()> {
        if (0xd800..=0xdbff).contains(&word) {
            ensure!(
                self.high_surrogate.replace(word).is_none(),
                "An RTF Unicode surrogate is incomplete."
            );
        } else if (0xdc00..=0xdfff).contains(&word) {
            let high = self
                .high_surrogate
                .take()
                .context("An RTF Unicode low surrogate has no high surrogate")?;
            let scalar = 0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(word) - 0xdc00;
            self.text(
                &char::from_u32(scalar)
                    .context("Invalid RTF Unicode character")?
                    .to_string(),
                state,
            );
        } else {
            ensure!(
                self.high_surrogate.is_none(),
                "An RTF Unicode surrogate is incomplete."
            );
            self.text(
                &char::from_u32(u32::from(word))
                    .context("Invalid RTF Unicode character")?
                    .to_string(),
                state,
            );
        }
        Ok(())
    }
    fn walk(&mut self, group: &[Token], mut state: RtfState) -> Result<()> {
        for token in group {
            match token {
                Token::Group(nested) => match destination(nested) {
                    Some("fonttbl") => {
                        for token in nested {
                            if let Token::Group(font) = token
                                && let Some(index) = words(font, "f")
                            {
                                self.fonts.insert(
                                    index,
                                    token_text(font)?.trim().trim_end_matches(';').to_owned(),
                                );
                            }
                        }
                    }
                    Some("colortbl") => {
                        self.colors.clear();
                        let (mut red, mut green, mut blue) = (0, 0, 0);
                        let mut explicit = false;
                        for token in nested {
                            match token {
                                Token::Word(word, Some(number)) if word == "red" => {
                                    red = (*number).clamp(0, 255);
                                    explicit = true;
                                }
                                Token::Word(word, Some(number)) if word == "green" => {
                                    green = (*number).clamp(0, 255);
                                    explicit = true;
                                }
                                Token::Word(word, Some(number)) if word == "blue" => {
                                    blue = (*number).clamp(0, 255);
                                    explicit = true;
                                }
                                Token::Text(bytes) => {
                                    for byte in bytes {
                                        if *byte == b';' {
                                            self.colors.push(explicit.then_some(
                                                ((red as u32) << 16)
                                                    | ((green as u32) << 8)
                                                    | blue as u32,
                                            ));
                                            red = 0;
                                            green = 0;
                                            blue = 0;
                                            explicit = false;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some("pict") => {
                        let picture = picture_from_rtf(nested)?;
                        self.current.runs.push(Run {
                            text: String::new(),
                            style: state.style.clone(),
                            picture: Some(picture),
                        });
                    }
                    Some("field") => {
                        let instruction = nested.iter().find_map(|token| match token {
                            Token::Group(value) if destination(value) == Some("fldinst") => {
                                Some(value)
                            }
                            _ => None,
                        });
                        let result = nested.iter().find_map(|token| match token {
                            Token::Group(value) if destination(value) == Some("fldrslt") => {
                                Some(value)
                            }
                            _ => None,
                        });
                        if let (Some(instruction), Some(result)) = (instruction, result) {
                            let instruction = token_text(instruction)?;
                            ensure!(
                                instruction.trim_start().starts_with("HYPERLINK"),
                                "This document contains a dynamic RTF field that cannot be converted without losing its behavior. Save as RTF instead."
                            );
                            let link = instruction
                                .trim_start()
                                .trim_start_matches("HYPERLINK")
                                .trim()
                                .trim_matches('"');
                            let mut linked = state.clone();
                            linked.style.link = Some(safe_link(link)?);
                            self.walk(result, linked)?;
                        }
                    }
                    Some("pn") => {
                        let list = if nested
                            .iter()
                            .any(|token| matches!(token, Token::Word(word,_) if word=="pnlvlblt"))
                        {
                            List::Bullet
                        } else {
                            List::Number
                        };
                        self.current.list = list;
                    }
                    Some("pntext" | "listtext") => {}
                    Some(
                        "object" | "footnote" | "annotation" | "header" | "footer" | "NeXTGraphic",
                    ) => bail!(
                        "The document contains an embedded object, note, header, or external attachment that cannot be converted safely. Save as RTF to retain it."
                    ),
                    Some(
                        "info" | "generator" | "stylesheet" | "listtable" | "listoverridetable"
                        | "nonshppict" | "datastore" | "xmlnstbl" | "colorschememapping",
                    ) => {}
                    _ if matches!(nested.first(), Some(Token::Word(word,_)) if word=="*") => {}
                    _ => self.walk(nested, state.clone())?,
                },
                Token::Text(bytes) => {
                    let skipped = self.skip.min(bytes.len());
                    self.skip -= skipped;
                    let (text, errors) =
                        WINDOWS_1252.decode_without_bom_handling(&bytes[skipped..]);
                    ensure!(
                        !errors,
                        "The RTF contains bytes that cannot be decoded without replacement."
                    );
                    self.text(&text, &state);
                }
                Token::Byte(byte) => {
                    if self.skip > 0 {
                        self.skip -= 1;
                    } else {
                        let input = [*byte];
                        let (text, errors) = WINDOWS_1252.decode_without_bom_handling(&input);
                        ensure!(!errors, "An RTF hexadecimal escape is invalid.");
                        self.text(&text, &state);
                    }
                }
                Token::Binary(_) => {
                    bail!("The document contains binary data outside a supported image.")
                }
                Token::Word(word, arg) => {
                    let n = arg.unwrap_or(1);
                    match word.as_str() {
                        "b" => state.style.bold = n != 0,
                        "i" => state.style.italic = n != 0,
                        "ul" => state.style.underline = n != 0,
                        "ulnone" => state.style.underline = false,
                        "strike" => state.style.strike = n != 0,
                        "super" => state.style.position = 1,
                        "sub" => state.style.position = -1,
                        "nosupersub" => state.style.position = 0,
                        "f" => {
                            if let Some(font) = self.fonts.get(&n) {
                                state.style.font = font.clone();
                            }
                        }
                        "fs" => state.style.size = n as f32 / 2.0,
                        "cf" => {
                            state.style.foreground = self
                                .colors
                                .get(n.max(0) as usize)
                                .copied()
                                .flatten()
                                .unwrap_or(0)
                        }
                        "highlight" | "cb" => {
                            state.style.background =
                                self.colors.get(n.max(0) as usize).copied().flatten()
                        }
                        "plain" => state.style = Style::default(),
                        "pard" => {
                            state.paragraph = Paragraph::default();
                            let runs = std::mem::take(&mut self.current.runs);
                            self.current = Paragraph {
                                runs,
                                ..state.paragraph.clone()
                            };
                        }
                        "ql" => self.current.alignment = Alignment::Left,
                        "qc" => self.current.alignment = Alignment::Center,
                        "qr" => self.current.alignment = Alignment::Right,
                        "qj" => self.current.alignment = Alignment::Justify,
                        "li" => self.current.left = n,
                        "ri" => self.current.right = n,
                        "fi" => self.current.first = n,
                        "sb" => self.current.before = n,
                        "sa" => self.current.after = n,
                        "sl" => self.current.line = n,
                        "ls" => self.current.list = List::Number,
                        "par" => self.paragraph(true),
                        "line" => self.text("\n", &state),
                        "tab" => self.text("\t", &state),
                        "u" => {
                            self.code_unit(n as i16 as u16, &state)?;
                            self.skip = state.unicode_skip;
                        }
                        "uc" => {
                            state.unicode_skip =
                                usize::try_from(n).context("Invalid RTF Unicode fallback count")?
                        }
                        "~" => self.text("\u{a0}", &state),
                        "_" => self.text("\u{2011}", &state),
                        "-" => self.text("\u{ad}", &state),
                        "emdash" => self.text("—", &state),
                        "endash" => self.text("–", &state),
                        "bullet" => self.text("•", &state),
                        "lquote" => self.text("‘", &state),
                        "rquote" => self.text("’", &state),
                        "ldblquote" => self.text("“", &state),
                        "rdblquote" => self.text("”", &state),
                        "trowd" => {
                            self.paragraph(false);
                            self.row_active = true;
                            self.row.clear();
                            self.cell.clear();
                        }
                        "intbl" => {}
                        "cell" => {
                            self.paragraph(self.cell.is_empty());
                            self.row.push(std::mem::take(&mut self.cell));
                        }
                        "row" => {
                            if !self.current.runs.is_empty() || !self.cell.is_empty() {
                                self.paragraph(false);
                                self.row.push(std::mem::take(&mut self.cell));
                            }
                            self.table.push(std::mem::take(&mut self.row));
                            self.row_active = false;
                            self.current = Paragraph::default();
                        }
                        "paperw" => self.document.width = n,
                        "paperh" => self.document.height = n,
                        "margl" => self.document.margins[0] = n,
                        "margr" => self.document.margins[1] = n,
                        "margt" => self.document.margins[2] = n,
                        "margb" => self.document.margins[3] = n,
                        "ansicpg" => ensure!(
                            n == 1252,
                            "This RTF code page cannot be converted without losing characters. Save it as RTF instead."
                        ),
                        "nesttableprops" | "nestcell" | "nestrow" => {
                            bail!("Nested tables cannot be converted safely; save as RTF instead.")
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}
fn read_rtf(bytes: &[u8]) -> Result<Document> {
    let parsed = tokens(bytes, &mut 0, 0)?;
    let mut reader = RtfReader::new();
    reader.walk(
        &parsed,
        RtfState {
            unicode_skip: 1,
            ..RtfState::default()
        },
    )?;
    ensure!(
        reader.high_surrogate.is_none(),
        "The RTF ends in an incomplete Unicode surrogate."
    );
    ensure!(
        !reader.row_active,
        "The RTF ends in an incomplete table row."
    );
    reader.paragraph(false);
    reader.finish_table();
    if reader.document.blocks.is_empty() {
        reader
            .document
            .blocks
            .push(Block::Paragraph(Paragraph::default()));
    }
    Ok(reader.document)
}

fn picture_from_rtf(group: &[Token]) -> Result<Picture> {
    let mut mime = None;
    let mut bytes = Vec::new();
    let mut hex = String::new();
    for token in group {
        match token {
            Token::Word(word, _) => match word.as_str() {
                "pngblip" => mime = Some("image/png"),
                "jpegblip" => mime = Some("image/jpeg"),
                "dibitmap" => mime = Some("image/bmp"),
                "wmetafile" => mime = Some("image/x-wmf"),
                _ => {}
            },
            Token::Text(text) => {
                for byte in text {
                    if !byte.is_ascii_whitespace() {
                        ensure!(
                            byte.is_ascii_hexdigit(),
                            "An RTF image contains invalid hexadecimal data."
                        );
                        hex.push(char::from(*byte));
                    }
                }
            }
            Token::Binary(data) => bytes.extend_from_slice(data),
            _ => {}
        }
    }
    ensure!(
        hex.len().is_multiple_of(2),
        "An RTF image ends in an incomplete hexadecimal byte."
    );
    for index in (0..hex.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&hex[index..index + 2], 16)?);
    }
    let mime = mime
        .context("This embedded picture format cannot be converted safely; save as RTF instead")?;
    if mime == "image/bmp" && !bytes.starts_with(b"BM") {
        ensure!(bytes.len() >= 40, "An RTF bitmap is incomplete.");
        let header = u32::from_le_bytes(bytes[0..4].try_into()?);
        ensure!(
            usize::try_from(header)? <= bytes.len(),
            "An RTF bitmap header is incomplete."
        );
        let bits = u16::from_le_bytes(bytes[14..16].try_into()?);
        let used = u32::from_le_bytes(bytes[32..36].try_into()?);
        let palette = if used > 0 {
            used
        } else if bits <= 8 {
            1_u32 << bits
        } else {
            0
        };
        let offset = 14_u32
            .checked_add(header)
            .and_then(|v| v.checked_add(palette.saturating_mul(4)))
            .context("Bitmap dimensions overflow")?;
        let mut file = b"BM".to_vec();
        file.extend_from_slice(&u32::try_from(bytes.len() + 14)?.to_le_bytes());
        file.extend_from_slice(&[0; 4]);
        file.extend_from_slice(&offset.to_le_bytes());
        file.extend_from_slice(&bytes);
        bytes = file;
    }
    ensure!(!bytes.is_empty(), "An embedded image has no data.");
    Ok(Picture {
        bytes,
        mime: mime.into(),
        width: words(group, "picwgoal").unwrap_or(2880).max(1),
        height: words(group, "pichgoal").unwrap_or(2160).max(1),
    })
}
fn rtf_escape(text: &str) -> String {
    let mut result = String::new();
    for unit in text.encode_utf16() {
        match unit {
            9 => result.push_str("\\tab "),
            10 => result.push_str("\\line "),
            13 => {}
            92 | 123 | 125 => {
                result.push('\\');
                result.push(char::from_u32(u32::from(unit)).expect("ASCII code unit"));
            }
            32..=126 => result.push(char::from_u32(u32::from(unit)).expect("ASCII code unit")),
            _ => {
                use std::fmt::Write as _;
                write!(result, "\\u{}?", unit as i16).expect("writing into a String cannot fail");
            }
        }
    }
    result
}
fn all_paragraphs(document: &Document) -> Vec<&Paragraph> {
    let mut result = Vec::new();
    for block in &document.blocks {
        match block {
            Block::Paragraph(p) => result.push(p),
            Block::Table(rows) => {
                for row in rows {
                    for cell in row {
                        result.extend(cell);
                    }
                }
            }
        }
    }
    result
}
fn write_rtf(document: &Document) -> Result<Vec<u8>> {
    let mut fonts = vec![String::from("Helvetica")];
    let mut colors = vec![0_u32];
    for paragraph in all_paragraphs(document) {
        for run in &paragraph.runs {
            if !fonts.contains(&run.style.font) {
                fonts.push(run.style.font.clone());
            }
            for color in [Some(run.style.foreground), run.style.background]
                .into_iter()
                .flatten()
            {
                if !colors.contains(&color) {
                    colors.push(color);
                }
            }
        }
    }
    let mut output = String::from("{\\rtf1\\ansi\\ansicpg1252\\uc1\\deff0{\\fonttbl");
    for (index, font) in fonts.iter().enumerate() {
        output.push_str(&format!("{{\\f{index}\\fnil {};}}", rtf_escape(font)));
    }
    output.push('}');
    output.push_str("{\\colortbl;");
    for color in &colors {
        output.push_str(&format!(
            "\\red{}\\green{}\\blue{};",
            color >> 16,
            (color >> 8) & 255,
            color & 255
        ));
    }
    output.push('}');
    output.push_str(&format!(
        "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}\n",
        document.width,
        document.height,
        document.margins[0],
        document.margins[1],
        document.margins[2],
        document.margins[3]
    ));
    for block in &document.blocks {
        match block {
            Block::Paragraph(paragraph) => {
                write_rtf_paragraph(paragraph, false, &fonts, &colors, &mut output)?
            }
            Block::Table(rows) => {
                for row in rows {
                    ensure!(!row.is_empty(), "An empty table row cannot be represented.");
                    output.push_str("\\trowd\\trgaph108");
                    let cell_width = (document.width - document.margins[0] - document.margins[1])
                        / i32::try_from(row.len())?;
                    for index in 1..=row.len() {
                        output.push_str(&format!("\\clbrdrt\\brdrs\\clbrdrl\\brdrs\\clbrdrb\\brdrs\\clbrdrr\\brdrs\\cellx{}",i32::try_from(index)?*cell_width));
                    }
                    for cell in row {
                        for paragraph in cell {
                            write_rtf_paragraph(paragraph, true, &fonts, &colors, &mut output)?;
                        }
                        output.push_str("\\cell ");
                    }
                    output.push_str("\\row\n");
                }
            }
        }
    }
    output.push('}');
    Ok(output.into_bytes())
}
fn write_rtf_paragraph(
    paragraph: &Paragraph,
    table: bool,
    fonts: &[String],
    colors: &[u32],
    output: &mut String,
) -> Result<()> {
    output.push_str("\\pard");
    if table {
        output.push_str("\\intbl");
    }
    output.push_str(match paragraph.alignment {
        Alignment::Left => "\\ql",
        Alignment::Center => "\\qc",
        Alignment::Right => "\\qr",
        Alignment::Justify => "\\qj",
    });
    output.push_str(&format!(
        "\\li{}\\ri{}\\fi{}\\sb{}\\sa{}",
        paragraph.left, paragraph.right, paragraph.first, paragraph.before, paragraph.after
    ));
    if paragraph.line != 0 {
        output.push_str(&format!("\\sl{}\\slmult0", paragraph.line));
    }
    match paragraph.list {
        List::None => {}
        List::Bullet => output.push_str("{\\*\\pn\\pnlvlblt\\pnf0{\\pntxtb\\u8226?}}"),
        List::Number => output.push_str("{\\*\\pn\\pnlvlbody\\pndec\\pnstart1{\\pntxta.}}"),
    }
    // A space ends a control word; after the list group's closing brace it would be text.
    if paragraph.list == List::None {
        output.push(' ');
    }
    for run in &paragraph.runs {
        let style = &run.style;
        let font = fonts
            .iter()
            .position(|font| *font == style.font)
            .unwrap_or(0);
        let fg = colors
            .iter()
            .position(|color| *color == style.foreground)
            .unwrap_or(0)
            + 1;
        let bg = style
            .background
            .and_then(|color| colors.iter().position(|candidate| *candidate == color))
            .map_or(0, |index| index + 1);
        output.push_str(&format!(
            "{{\\f{font}\\fs{}\\b{}\\i{}\\ul{}\\strike{}\\cf{fg}\\highlight{bg}",
            (style.size * 2.0).round() as i32,
            i32::from(style.bold),
            i32::from(style.italic),
            i32::from(style.underline),
            i32::from(style.strike)
        ));
        output.push_str(match style.position {
            1 => "\\super ",
            -1 => "\\sub ",
            _ => "\\nosupersub ",
        });
        if let Some(link) = &style.link {
            output.push_str(&format!(
                "{{\\field{{\\*\\fldinst HYPERLINK \"{}\"}}{{\\fldrslt ",
                rtf_escape(link)
            ));
        }
        if let Some(picture) = &run.picture {
            let kind = match picture.mime.as_str() {
                "image/png" => "pngblip",
                "image/jpeg" => "jpegblip",
                "image/bmp" => "dibitmap0",
                "image/x-wmf" => "wmetafile8",
                _ => bail!("The document contains an unsupported image type"),
            };
            output.push_str(&format!(
                "{{\\pict\\{kind}\\picwgoal{}\\pichgoal{} ",
                picture.width, picture.height
            ));
            let bytes = if picture.mime == "image/bmp" && picture.bytes.starts_with(b"BM") {
                &picture.bytes[14..]
            } else {
                &picture.bytes
            };
            for byte in bytes {
                use std::fmt::Write as _;
                write!(output, "{byte:02x}").expect("writing into a String cannot fail");
            }
            output.push('}');
        } else {
            output.push_str(&rtf_escape(&run.text));
        }
        if style.link.is_some() {
            output.push_str("}}");
        }
        output.push('}');
    }
    output.push_str("\\par\n");
    Ok(())
}

fn zip_part(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>> {
    let mut part = archive
        .by_name(name)
        .with_context(|| format!("The document package is missing {name}"))?;
    ensure!(
        part.size() <= MAX_PART,
        "A document package part exceeds the 128 MiB safety limit."
    );
    let mut bytes = Vec::new();
    part.by_ref().take(MAX_PART + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_PART,
        "A decompressed document part exceeds the safety limit."
    );
    Ok(bytes)
}
fn optional_zip_text(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>> {
    match archive.by_name(name) {
        Ok(part) => drop(part),
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(String::from_utf8(zip_part(archive, name)?)?))
}
fn package_image(bytes: Vec<u8>, name: &str, width: i32, height: i32) -> Result<Picture> {
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[255, 216, 255]) {
        "image/jpeg"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if name.to_ascii_lowercase().ends_with(".wmf") {
        "image/x-wmf"
    } else {
        bail!(
            "The document contains an unsupported image format: {name}. No file has been modified."
        )
    };
    Ok(Picture {
        bytes,
        mime: mime.into(),
        width: width.max(1),
        height: height.max(1),
    })
}
fn package_target(base: &str, target: &str) -> Result<String> {
    ensure!(
        !target.contains(':') && !target.starts_with('/'),
        "The document contains an external package attachment."
    );
    let joined = format!("{base}/{target}");
    let mut parts = Vec::new();
    for component in joined.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                ensure!(
                    parts.pop().is_some(),
                    "An attachment points outside its document package."
                );
            }
            _ => parts.push(component),
        }
    }
    Ok(parts.join("/"))
}
#[derive(Default)]
struct Relationships {
    targets: BTreeMap<String, String>,
    external: BTreeMap<String, String>,
}
fn read_relationships(text: &str) -> Result<Relationships> {
    let xml = XmlDocument::parse(text)?;
    let mut result = Relationships::default();
    for relationship in xml
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "Relationship")
    {
        let (Some(id), Some(target)) = (value(relationship, "Id"), value(relationship, "Target"))
        else {
            continue;
        };
        if value(relationship, "TargetMode") == Some("External") {
            result.external.insert(id.into(), target.into());
        } else {
            result
                .targets
                .insert(id.into(), package_target("word", target)?);
        }
    }
    Ok(result)
}
fn word_style(properties: Node<'_, '_>, style: &mut Style) {
    for property in properties.children().filter(Node::is_element) {
        match property.tag_name().name() {
            "b" => style.bold = on(property),
            "i" => style.italic = on(property),
            "u" => style.underline = on(property),
            "strike" => style.strike = on(property),
            "rFonts" => {
                if let Some(font) = value(property, "ascii").or_else(|| value(property, "hAnsi")) {
                    style.font = font.into();
                }
            }
            "sz" => style.size = number(property, "val", 24) as f32 / 2.0,
            "color" => {
                if let Some(color) = value(property, "val").and_then(color) {
                    style.foreground = color;
                }
            }
            "shd" => style.background = value(property, "fill").and_then(color),
            "highlight" => style.background = value(property, "val").and_then(color),
            "vertAlign" => {
                style.position = match value(property, "val") {
                    Some("superscript") => 1,
                    Some("subscript") => -1,
                    _ => 0,
                }
            }
            _ => {}
        }
    }
}
fn validate_archive(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<()> {
    ensure!(
        archive.len() <= 10_000,
        "The document package contains too many files."
    );
    let mut expanded = 0_u64;
    for index in 0..archive.len() {
        expanded = expanded
            .checked_add(archive.by_index(index)?.size())
            .context("The expanded document package is too large")?;
        ensure!(
            expanded <= MAX_PART * 4,
            "The expanded document package exceeds the 512 MiB safety limit."
        );
    }
    Ok(())
}
fn read_word_numbering(text: &str) -> Result<BTreeMap<String, List>> {
    let document = XmlDocument::parse(text)?;
    let mut definitions = BTreeMap::new();
    let mut result = BTreeMap::new();
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "abstractNum")
    {
        if let Some(id) = value(node, "abstractNumId") {
            let kind = if node.descendants().any(|node| {
                node.is_element()
                    && node.tag_name().name() == "numFmt"
                    && value(node, "val") == Some("bullet")
            }) {
                List::Bullet
            } else {
                List::Number
            };
            definitions.insert(id.to_owned(), kind);
        }
    }
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "num")
    {
        if let (Some(id), Some(definition)) = (
            value(node, "numId"),
            child(node, "abstractNumId").and_then(|node| value(node, "val")),
        ) {
            result.insert(
                id.to_owned(),
                definitions.get(definition).copied().unwrap_or(List::Number),
            );
        }
    }
    Ok(result)
}
fn read_docx(bytes: &[u8]) -> Result<Document> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).context("Opening the Word document package")?;
    validate_archive(&mut archive)?;
    let text = String::from_utf8(zip_part(&mut archive, "word/document.xml")?)?;
    let xml = XmlDocument::parse(&text)?;
    let relationships = optional_zip_text(&mut archive, "word/_rels/document.xml.rels")?
        .map(|text| read_relationships(&text))
        .transpose()?
        .unwrap_or_default();
    let numbers = optional_zip_text(&mut archive, "word/numbering.xml")?
        .map(|text| read_word_numbering(&text))
        .transpose()?
        .unwrap_or_default();
    let mut defaults = Style::default();
    let mut styles = BTreeMap::new();
    if let Some(text) = optional_zip_text(&mut archive, "word/styles.xml")? {
        let styles_xml = XmlDocument::parse(&text)?;
        if let Some(properties) = styles_xml
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == "rPrDefault")
            .and_then(|node| child(node, "rPr"))
        {
            word_style(properties, &mut defaults);
        }
        for node in styles_xml
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "style")
        {
            if let Some(id) = value(node, "styleId") {
                let mut style = defaults.clone();
                if let Some(properties) = child(node, "rPr") {
                    word_style(properties, &mut style);
                }
                styles.insert(id.to_owned(), style);
            }
        }
    }
    let mut document = Document::default();
    let body = xml
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "body")
        .context("The Word document has no body")?;
    for node in body.descendants().filter(|node| {
        node.is_element()
            && matches!(node.tag_name().name(), "p" | "tbl" | "sectPr")
            && !node
                .ancestors()
                .skip(1)
                .take_while(|ancestor| *ancestor != body)
                .any(|ancestor| {
                    ancestor.is_element() && matches!(ancestor.tag_name().name(), "p" | "tbl")
                })
    }) {
        match node.tag_name().name() {
            "p" => document.blocks.push(Block::Paragraph(word_paragraph(
                node,
                &defaults,
                &styles,
                &numbers,
                &relationships,
                &mut archive,
            )?)),
            "tbl" => {
                let mut rows = Vec::new();
                for row in node
                    .children()
                    .filter(|node| node.is_element() && node.tag_name().name() == "tr")
                {
                    let mut cells = Vec::new();
                    for cell in row
                        .children()
                        .filter(|node| node.is_element() && node.tag_name().name() == "tc")
                    {
                        ensure!(
                            !cell.descendants().any(|node| node.is_element()
                                && matches!(
                                    node.tag_name().name(),
                                    "gridSpan" | "vMerge" | "hMerge"
                                )),
                            "Merged Word table cells cannot be imported without changing their layout. The original is unchanged."
                        );
                        let mut paragraphs = Vec::new();
                        for content in cell.children().filter(Node::is_element) {
                            match content.tag_name().name() {
                                "p" => paragraphs.push(word_paragraph(
                                    content,
                                    &defaults,
                                    &styles,
                                    &numbers,
                                    &relationships,
                                    &mut archive,
                                )?),
                                "tbl" => bail!("Nested Word tables cannot be imported safely."),
                                _ => {}
                            }
                        }
                        cells.push(paragraphs);
                    }
                    rows.push(cells);
                }
                document.blocks.push(Block::Table(rows));
            }
            "sectPr" => {
                if let Some(size) = child(node, "pgSz") {
                    document.width = number(size, "w", document.width);
                    document.height = number(size, "h", document.height);
                }
                if let Some(margins) = child(node, "pgMar") {
                    document.margins = [
                        number(margins, "left", 1440),
                        number(margins, "right", 1440),
                        number(margins, "top", 1440),
                        number(margins, "bottom", 1440),
                    ];
                }
            }
            _ => {}
        }
    }
    Ok(document)
}
fn word_paragraph(
    node: Node<'_, '_>,
    defaults: &Style,
    styles: &BTreeMap<String, Style>,
    numbers: &BTreeMap<String, List>,
    relationships: &Relationships,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<Paragraph> {
    let mut result = Paragraph::default();
    let mut style = defaults.clone();
    if let Some(properties) = child(node, "pPr") {
        if let Some(id) = child(properties, "pStyle").and_then(|node| value(node, "val"))
            && let Some(stored) = styles.get(id)
        {
            style = stored.clone();
        }
        for property in properties.children().filter(Node::is_element) {
            match property.tag_name().name() {
                "jc" => {
                    result.alignment = match value(property, "val") {
                        Some("center") => Alignment::Center,
                        Some("right" | "end") => Alignment::Right,
                        Some("both" | "distribute") => Alignment::Justify,
                        _ => Alignment::Left,
                    }
                }
                "ind" => {
                    result.left = number(property, "left", 0);
                    result.right = number(property, "right", 0);
                    result.first =
                        number(property, "firstLine", 0) - number(property, "hanging", 0);
                }
                "spacing" => {
                    result.before = number(property, "before", 0);
                    result.after = number(property, "after", 0);
                    result.line = if value(property, "lineRule") == Some("exact") {
                        number(property, "line", 0)
                    } else {
                        0
                    };
                }
                "numPr" => {
                    result.list = child(property, "numId")
                        .and_then(|node| value(node, "val"))
                        .and_then(|id| numbers.get(id))
                        .copied()
                        .unwrap_or(List::Number)
                }
                _ => {}
            }
        }
    }
    word_inline(node, &style, &mut result, relationships, archive)?;
    Ok(result)
}
fn word_inline(
    node: Node<'_, '_>,
    style: &Style,
    paragraph: &mut Paragraph,
    relationships: &Relationships,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<()> {
    for element in node.children().filter(Node::is_element) {
        match element.tag_name().name() {
            "r" => {
                let mut style = style.clone();
                if let Some(properties) = child(element, "rPr") {
                    word_style(properties, &mut style);
                }
                word_inline(element, &style, paragraph, relationships, archive)?;
            }
            "t" => push_text(paragraph, element.text().unwrap_or(""), style),
            "tab" => push_text(paragraph, "\t", style),
            "br" | "cr" => push_text(paragraph, "\n", style),
            "hyperlink" => {
                let mut style = style.clone();
                if let Some(id) = value(element, "id") {
                    let url = relationships
                        .external
                        .get(id)
                        .context("A Word hyperlink has no destination")?;
                    style.link = Some(safe_link(url)?);
                } else if let Some(anchor) = value(element, "anchor") {
                    style.link = Some(format!("#{anchor}"));
                }
                word_inline(element, &style, paragraph, relationships, archive)?;
            }
            "drawing" | "pict" => {
                let image = element
                    .descendants()
                    .find(|node| {
                        node.is_element() && matches!(node.tag_name().name(), "blip" | "imagedata")
                    })
                    .context("The Word drawing is not a supported inline image")?;
                let id = value(image, "embed")
                    .or_else(|| value(image, "id"))
                    .context("The Word image has no attachment relationship")?;
                let target = relationships
                    .targets
                    .get(id)
                    .context("An external or missing Word image cannot be imported offline")?;
                let extent = element
                    .descendants()
                    .find(|node| node.is_element() && node.tag_name().name() == "extent");
                let width = extent.map_or(2880, |node| number(node, "cx", 1828800) / 635);
                let height = extent.map_or(2160, |node| number(node, "cy", 1371600) / 635);
                let picture = package_image(zip_part(archive, target)?, target, width, height)?;
                paragraph.runs.push(Run {
                    text: String::new(),
                    style: style.clone(),
                    picture: Some(picture),
                });
            }
            "object" => bail!(
                "The Word document contains an embedded object that cannot be imported safely."
            ),
            "ins" | "smartTag" | "sdt" | "sdtContent" => {
                word_inline(element, style, paragraph, relationships, archive)?
            }
            "fldChar" | "instrText" => bail!(
                "The Word document contains a dynamic field that cannot be imported without changing its behavior."
            ),
            _ => {}
        }
    }
    Ok(())
}

fn picture_extension(picture: &Picture) -> Result<&'static str> {
    match picture.mime.as_str() {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/bmp" => Ok("bmp"),
        "image/x-wmf" => Ok("wmf"),
        _ => bail!("Unsupported embedded image type"),
    }
}
fn add_zip(
    writer: &mut ZipWriter<Cursor<Vec<u8>>>,
    name: &str,
    bytes: &[u8],
    stored: bool,
) -> Result<()> {
    let compression = if stored {
        zip::CompressionMethod::Stored
    } else {
        zip::CompressionMethod::Deflated
    };
    writer.start_file(
        name,
        SimpleFileOptions::default().compression_method(compression),
    )?;
    writer.write_all(bytes)?;
    Ok(())
}
#[derive(Default)]
struct PackageOutput {
    pictures: Vec<Picture>,
    links: Vec<String>,
}
fn write_word_paragraph(paragraph: &Paragraph, package: &mut PackageOutput) -> Result<String> {
    let alignment = match paragraph.alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Justify => "both",
    };
    let mut output = format!(
        "<w:p><w:pPr><w:jc w:val=\"{alignment}\"/><w:ind w:left=\"{}\" w:right=\"{}\" w:firstLine=\"{}\"/><w:spacing w:before=\"{}\" w:after=\"{}\"",
        paragraph.left, paragraph.right, paragraph.first, paragraph.before, paragraph.after
    );
    if paragraph.line != 0 {
        output.push_str(&format!(
            " w:line=\"{}\" w:lineRule=\"exact\"",
            paragraph.line.abs()
        ));
    }
    output.push_str("/>");
    if paragraph.list != List::None {
        output.push_str(&format!(
            "<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"{}\"/></w:numPr>",
            if paragraph.list == List::Bullet { 1 } else { 2 }
        ));
    }
    output.push_str("</w:pPr>");
    for run in &paragraph.runs {
        let style = &run.style;
        if let Some(link) = &style.link {
            package.links.push(link.clone());
            output.push_str(&format!(
                "<w:hyperlink r:id=\"link{}\">",
                package.links.len()
            ));
        }
        output.push_str(&format!("<w:r><w:rPr><w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/><w:sz w:val=\"{}\"/><w:color w:val=\"{:06X}\"/>",xml(&style.font),xml(&style.font),(style.size*2.0).round() as i32,style.foreground));
        if style.bold {
            output.push_str("<w:b/>");
        }
        if style.italic {
            output.push_str("<w:i/>");
        }
        if style.underline {
            output.push_str("<w:u w:val=\"single\"/>");
        }
        if style.strike {
            output.push_str("<w:strike/>");
        }
        if let Some(color) = style.background {
            output.push_str(&format!("<w:shd w:val=\"clear\" w:fill=\"{color:06X}\"/>"));
        }
        if style.position != 0 {
            output.push_str(&format!(
                "<w:vertAlign w:val=\"{}\"/>",
                if style.position > 0 {
                    "superscript"
                } else {
                    "subscript"
                }
            ));
        }
        output.push_str("</w:rPr>");
        if let Some(picture) = &run.picture {
            package.pictures.push(picture.clone());
            let id = package.pictures.len();
            let cx = i64::from(picture.width) * 635;
            let cy = i64::from(picture.height) * 635;
            output.push_str(&format!("<w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"Picture {id}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"image{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"));
        } else {
            let mut text = String::new();
            for character in run.text.chars() {
                match character {
                    '\t' | '\n' => {
                        if !text.is_empty() {
                            output.push_str(&format!(
                                "<w:t xml:space=\"preserve\">{}</w:t>",
                                xml(&text)
                            ));
                            text.clear();
                        }
                        output.push_str(if character == '\t' {
                            "<w:tab/>"
                        } else {
                            "<w:br/>"
                        });
                    }
                    _ => text.push(character),
                }
            }
            if !text.is_empty() {
                output.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", xml(&text)));
            }
        }
        output.push_str("</w:r>");
        if style.link.is_some() {
            output.push_str("</w:hyperlink>");
        }
    }
    output.push_str("</w:p>");
    Ok(output)
}
fn write_docx(document: &Document) -> Result<Vec<u8>> {
    let mut package = PackageOutput::default();
    let mut body = String::new();
    for block in &document.blocks {
        match block {
            Block::Paragraph(paragraph) => {
                body.push_str(&write_word_paragraph(paragraph, &mut package)?)
            }
            Block::Table(rows) => {
                body.push_str("<w:tbl><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\"/><w:left w:val=\"single\" w:sz=\"4\"/><w:bottom w:val=\"single\" w:sz=\"4\"/><w:right w:val=\"single\" w:sz=\"4\"/><w:insideH w:val=\"single\" w:sz=\"4\"/><w:insideV w:val=\"single\" w:sz=\"4\"/></w:tblBorders></w:tblPr>");
                for row in rows {
                    body.push_str("<w:tr>");
                    for cell in row {
                        body.push_str("<w:tc>");
                        for paragraph in cell {
                            body.push_str(&write_word_paragraph(paragraph, &mut package)?);
                        }
                        if cell.is_empty() {
                            body.push_str("<w:p/>");
                        }
                        body.push_str("</w:tc>");
                    }
                    body.push_str("</w:tr>");
                }
                body.push_str("</w:tbl>");
            }
        }
    }
    body.push_str(&format!("<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:left=\"{}\" w:right=\"{}\" w:top=\"{}\" w:bottom=\"{}\"/></w:sectPr>",document.width,document.height,document.margins[0],document.margins[1],document.margins[2],document.margins[3]));
    let xml_document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{body}</w:body></w:document>"
    );
    let mut types = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>",
    );
    let mut content_types = BTreeMap::new();
    for picture in &package.pictures {
        content_types.insert(picture_extension(picture)?, picture.mime.as_str());
    }
    for (extension, mime) in content_types {
        types.push_str(&format!(
            "<Default Extension=\"{extension}\" ContentType=\"{mime}\"/>"
        ));
    }
    types.push_str("</Types>");
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"numbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>",
    );
    for (index, link) in package.links.iter().enumerate() {
        rels.push_str(&format!("<Relationship Id=\"link{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",index+1,xml(link)));
    }
    for (index, picture) in package.pictures.iter().enumerate() {
        rels.push_str(&format!("<Relationship Id=\"image{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image{}.{}\"/>",index+1,index+1,picture_extension(picture)?));
    }
    rels.push_str("</Relationships>");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    add_zip(&mut writer, "[Content_Types].xml", types.as_bytes(), false)?;
    add_zip(&mut writer,"_rels/.rels",br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="document" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,false)?;
    add_zip(
        &mut writer,
        "word/document.xml",
        xml_document.as_bytes(),
        false,
    )?;
    add_zip(
        &mut writer,
        "word/_rels/document.xml.rels",
        rels.as_bytes(),
        false,
    )?;
    add_zip(&mut writer,"word/numbering.xml",br#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="&#x2022;"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="2"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="1"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="2"/></w:num></w:numbering>"#,false)?;
    for (index, picture) in package.pictures.iter().enumerate() {
        add_zip(
            &mut writer,
            &format!(
                "word/media/image{}.{}",
                index + 1,
                picture_extension(picture)?
            ),
            &picture.bytes,
            false,
        )?;
    }
    Ok(writer.finish()?.into_inner())
}

fn css_style(style: &Style) -> String {
    let mut css = format!(
        "font-family:'{}';font-size:{}pt;color:#{:06x};font-weight:{};font-style:{};",
        style.font.replace('\'', ""),
        style.size,
        style.foreground,
        if style.bold { "bold" } else { "normal" },
        if style.italic { "italic" } else { "normal" }
    );
    if style.underline || style.strike {
        css.push_str(&format!(
            "text-decoration:{} {};",
            if style.underline { "underline" } else { "" },
            if style.strike { "line-through" } else { "" }
        ));
    }
    if let Some(color) = style.background {
        css.push_str(&format!("background-color:#{color:06x};"));
    }
    if style.position != 0 {
        css.push_str(if style.position > 0 {
            "vertical-align:super;"
        } else {
            "vertical-align:sub;"
        });
    }
    css
}
fn write_html_paragraph(paragraph: &Paragraph) -> Result<String> {
    let alignment = match paragraph.alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Justify => "justify",
    };
    let mut output = format!(
        "<p style=\"text-align:{alignment};margin-left:{}pt;margin-right:{}pt;text-indent:{}pt;margin-top:{}pt;margin-bottom:{}pt;white-space:pre-wrap;\">",
        paragraph.left as f32 / 20.0,
        paragraph.right as f32 / 20.0,
        paragraph.first as f32 / 20.0,
        paragraph.before as f32 / 20.0,
        paragraph.after as f32 / 20.0
    );
    for run in &paragraph.runs {
        if let Some(link) = &run.style.link {
            output.push_str(&format!("<a href=\"{}\">", xml(link)));
        }
        output.push_str(&format!("<span style=\"{}\">", xml(&css_style(&run.style))));
        if let Some(picture) = &run.picture {
            ensure!(
                picture.mime != "image/x-wmf",
                "Windows metafile images cannot be represented in standalone HTML without rasterization. Save as RTF, DOCX, or ODT instead."
            );
            output.push_str(&format!(
                "<img alt=\"\" src=\"data:{};base64,{}\" style=\"width:{}pt;height:{}pt;\">",
                picture.mime,
                STANDARD.encode(&picture.bytes),
                picture.width as f32 / 20.0,
                picture.height as f32 / 20.0
            ));
        } else {
            output.push_str(&xml(&run.text).replace('\n', "<br>"));
        }
        output.push_str("</span>");
        if run.style.link.is_some() {
            output.push_str("</a>");
        }
    }
    if paragraph.runs.is_empty() {
        output.push_str("<br>");
    }
    output.push_str("</p>");
    Ok(output)
}
fn write_html(document: &Document) -> Result<String> {
    let mut output = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Document</title></head><body>",
    );
    let mut list = List::None;
    for block in &document.blocks {
        if let Block::Paragraph(paragraph) = block {
            if list != paragraph.list {
                if list != List::None {
                    output.push_str(if list == List::Bullet {
                        "</ul>"
                    } else {
                        "</ol>"
                    });
                }
                list = paragraph.list;
                if list != List::None {
                    output.push_str(if list == List::Bullet { "<ul>" } else { "<ol>" });
                }
            }
            if list != List::None {
                output.push_str("<li>");
            }
            output.push_str(&write_html_paragraph(paragraph)?);
            if list != List::None {
                output.push_str("</li>");
            }
        } else if let Block::Table(rows) = block {
            if list != List::None {
                output.push_str(if list == List::Bullet {
                    "</ul>"
                } else {
                    "</ol>"
                });
                list = List::None;
            }
            output.push_str("<table style=\"border-collapse:collapse\" border=\"1\">");
            for row in rows {
                output.push_str("<tr>");
                for cell in row {
                    output.push_str("<td>");
                    for paragraph in cell {
                        output.push_str(&write_html_paragraph(paragraph)?);
                    }
                    output.push_str("</td>");
                }
                output.push_str("</tr>");
            }
            output.push_str("</table>");
        }
    }
    if list != List::None {
        output.push_str(if list == List::Bullet {
            "</ul>"
        } else {
            "</ol>"
        });
    }
    output.push_str("</body></html>");
    Ok(output)
}

fn length_twips(text: &str) -> Option<i32> {
    let text = text.trim();
    let split = text
        .find(|character: char| {
            !character.is_ascii_digit() && !matches!(character, '.' | '-' | '+')
        })
        .unwrap_or(text.len());
    let number = text[..split].parse::<f32>().ok()?;
    let factor = match text[split..].trim() {
        "pt" => 20.0,
        "px" | "" => 15.0,
        "in" => 1440.0,
        "cm" => 1440.0 / 2.54,
        "mm" => 144.0 / 2.54,
        _ => return None,
    };
    (number.is_finite() && number.abs() < 100_000.0).then_some((number * factor).round() as i32)
}
fn apply_css(css: &str, style: &mut Style, paragraph: &mut Paragraph) {
    for declaration in css.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "font-family" => {
                style.font = value
                    .split(',')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .trim_matches(['\'', '"'])
                    .into()
            }
            "font-size" => {
                if let Some(size) = length_twips(value) {
                    style.size = size as f32 / 20.0;
                }
            }
            "font-weight" => {
                style.bold =
                    value == "bold" || value.parse::<i32>().is_ok_and(|weight| weight >= 600)
            }
            "font-style" => style.italic = matches!(value, "italic" | "oblique"),
            "text-decoration" | "text-decoration-line" => {
                style.underline = value.contains("underline");
                style.strike = value.contains("line-through");
            }
            "color" => {
                if let Some(color) = color(value) {
                    style.foreground = color;
                }
            }
            "background" | "background-color" => style.background = color(value),
            "vertical-align" => {
                style.position = match value {
                    "super" => 1,
                    "sub" => -1,
                    _ => 0,
                }
            }
            "text-align" => {
                paragraph.alignment = match value {
                    "center" => Alignment::Center,
                    "right" | "end" => Alignment::Right,
                    "justify" => Alignment::Justify,
                    _ => Alignment::Left,
                }
            }
            "margin-left" => {
                if let Some(value) = length_twips(value) {
                    paragraph.left = value;
                }
            }
            "margin-right" => {
                if let Some(value) = length_twips(value) {
                    paragraph.right = value;
                }
            }
            "text-indent" => {
                if let Some(value) = length_twips(value) {
                    paragraph.first = value;
                }
            }
            "margin-top" => {
                if let Some(value) = length_twips(value) {
                    paragraph.before = value;
                }
            }
            "margin-bottom" => {
                if let Some(value) = length_twips(value) {
                    paragraph.after = value;
                }
            }
            "line-height" => {
                if let Some(value) = length_twips(value) {
                    paragraph.line = value;
                }
            }
            _ => {}
        }
    }
}
fn html_style(element: ElementRef<'_>, base: &Style, paragraph: &mut Paragraph) -> Style {
    let mut style = base.clone();
    match element.value().name() {
        "b" | "strong" => style.bold = true,
        "i" | "em" => style.italic = true,
        "u" => style.underline = true,
        "s" | "strike" | "del" => style.strike = true,
        "sup" => style.position = 1,
        "sub" => style.position = -1,
        // Menlo is not on Windows; the bundled monospace font is.
        "code" | "pre" => style.font = crate::fonts::PLAIN_FONT.into(),
        "h1" => {
            style.bold = true;
            style.size = 24.0;
        }
        "h2" => {
            style.bold = true;
            style.size = 20.0;
        }
        "h3" => {
            style.bold = true;
            style.size = 16.0;
        }
        "h4" => {
            style.bold = true;
            style.size = 14.0;
        }
        "h5" | "h6" => style.bold = true,
        _ => {}
    }
    if let Some(font) = element.value().attr("face") {
        style.font = font.into();
    }
    if let Some(color) = element.value().attr("color").and_then(color) {
        style.foreground = color;
    }
    if let Some(align) = element.value().attr("align") {
        paragraph.alignment = match align {
            "center" => Alignment::Center,
            "right" => Alignment::Right,
            "justify" => Alignment::Justify,
            _ => Alignment::Left,
        };
    }
    if let Some(css) = element.value().attr("style") {
        apply_css(css, &mut style, paragraph);
    }
    style
}
#[derive(Clone, Copy)]
enum HtmlLinks {
    Strict,
    Preview,
}
impl HtmlLinks {
    fn read(self, link: &str) -> Result<Option<String>> {
        match safe_link(link) {
            Ok(link) => Ok(Some(link)),
            // Preview retains the source; an inactive label must not reject the document.
            Err(_) if matches!(self, Self::Preview) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
fn read_html(bytes: &[u8], directory: &Path) -> Result<Document> {
    parse_html(bytes, directory, HtmlLinks::Strict)
}
fn parse_html(bytes: &[u8], directory: &Path, links: HtmlLinks) -> Result<Document> {
    let (text, encoding, errors) = encoding_rs::Encoding::for_bom(bytes)
        .map(|(encoding, skip)| {
            let (text, _, errors) = encoding.decode(&bytes[skip..]);
            (text, encoding, errors)
        })
        .unwrap_or_else(|| {
            let (text, _, errors) = encoding_rs::UTF_8.decode(bytes);
            (text, encoding_rs::UTF_8, errors)
        });
    let _ = encoding;
    ensure!(
        !errors,
        "The HTML document is not valid Unicode; reopen it using its original encoding."
    );
    let html = Html::parse_document(&text);
    let selector = scraper::Selector::parse("body").expect("constant selector");
    let body = html
        .select(&selector)
        .next()
        .context("The HTML document has no body")?;
    let mut document = Document::default();
    html_blocks(
        body,
        &Style::default(),
        directory,
        &mut document.blocks,
        List::None,
        links,
    )?;
    if document.blocks.is_empty() {
        document.blocks.push(Block::Paragraph(Paragraph::default()));
    }
    Ok(document)
}
/// Nesting deeper than this is refused instead of overflowing the stack.
const MAX_HTML_DEPTH: usize = 128;

fn html_blocks(
    element: ElementRef<'_>,
    style: &Style,
    directory: &Path,
    blocks: &mut Vec<Block>,
    list: List,
    links: HtmlLinks,
) -> Result<()> {
    ensure!(
        element.ancestors().count() <= MAX_HTML_DEPTH,
        "The HTML document is nested too deeply to import safely."
    );
    // Nested lists step in; the outermost list keeps the left margin.
    let depth = std::iter::once(element)
        .chain(element.ancestors().filter_map(ElementRef::wrap))
        .filter(|ancestor| matches!(ancestor.value().name(), "ul" | "ol"))
        .count();
    let left = if list == List::None {
        0
    } else {
        360 * (depth.max(1) as i32 - 1)
    };
    let mut pending = Paragraph {
        list,
        left,
        ..Paragraph::default()
    };
    for node in element.children() {
        if let Some(child) = ElementRef::wrap(node) {
            match child.value().name() {
                "script" | "style" | "head" | "noscript" => {}
                // A list item holding nested lists or paragraphs is a block container.
                "li" if child.children().filter_map(ElementRef::wrap).any(|inner| {
                    matches!(
                        inner.value().name(),
                        "ul" | "ol" | "table" | "p" | "pre" | "blockquote" | "div"
                    )
                }) =>
                {
                    flush(&mut pending, blocks);
                    html_blocks(child, style, directory, blocks, list, links)?;
                }
                "p" | "pre" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" => {
                    flush(&mut pending, blocks);
                    let mut paragraph = Paragraph {
                        list,
                        left,
                        ..Paragraph::default()
                    };
                    let style = html_style(child, style, &mut paragraph);
                    html_inline(child, &style, directory, &mut paragraph, links)?;
                    blocks.push(Block::Paragraph(paragraph));
                }
                "div" | "section" | "article" | "main" | "blockquote" => {
                    flush(&mut pending, blocks);
                    let mut paragraph = Paragraph::default();
                    let style = html_style(child, style, &mut paragraph);
                    html_blocks(child, &style, directory, blocks, list, links)?;
                }
                "ul" | "ol" => {
                    flush(&mut pending, blocks);
                    html_blocks(
                        child,
                        style,
                        directory,
                        blocks,
                        if child.value().name() == "ul" {
                            List::Bullet
                        } else {
                            List::Number
                        },
                        links,
                    )?;
                }
                "table" => {
                    flush(&mut pending, blocks);
                    let mut rows = Vec::new();
                    let row_selector = scraper::Selector::parse("tr").expect("constant selector");
                    for row in child.select(&row_selector) {
                        let mut cells = Vec::new();
                        for cell in row
                            .children()
                            .filter_map(ElementRef::wrap)
                            .filter(|element| matches!(element.value().name(), "td" | "th"))
                        {
                            ensure!(
                                cell.value().attr("colspan").unwrap_or("1") == "1"
                                    && cell.value().attr("rowspan").unwrap_or("1") == "1",
                                "Merged HTML table cells cannot be imported without changing their layout."
                            );
                            let mut content = Vec::new();
                            let mut cell_style = style.clone();
                            if cell.value().name() == "th" {
                                cell_style.bold = true;
                            }
                            html_blocks(
                                cell,
                                &cell_style,
                                directory,
                                &mut content,
                                List::None,
                                links,
                            )?;
                            let mut paragraphs = Vec::new();
                            for block in content {
                                match block {
                                    Block::Paragraph(paragraph) => paragraphs.push(paragraph),
                                    Block::Table(_) => {
                                        bail!("Nested HTML tables cannot be imported safely.")
                                    }
                                }
                            }
                            cells.push(paragraphs);
                        }
                        rows.push(cells);
                    }
                    blocks.push(Block::Table(rows));
                }
                _ => {
                    let style = html_style(child, style, &mut pending);
                    html_inline(child, &style, directory, &mut pending, links)?;
                }
            }
        } else if let Some(text) = node.value().as_text() {
            let text = text.text.to_string();
            if !text.trim().is_empty() || !pending.runs.is_empty() {
                push_text(&mut pending, &text, style);
            }
        }
    }
    flush(&mut pending, blocks);
    Ok(())
}
/// Ends the paragraph collected so far, keeping its list placement for the text after it.
/// Whitespace before a nested block is source formatting, not a blank line.
fn flush(pending: &mut Paragraph, blocks: &mut Vec<Block>) {
    if let Some(last) = pending.runs.last_mut()
        && last.picture.is_none()
    {
        last.text.truncate(last.text.trim_end().len());
    }
    pending
        .runs
        .retain(|run| run.picture.is_some() || !run.text.is_empty());
    if !pending.runs.is_empty() {
        let next = Paragraph {
            list: pending.list,
            left: pending.left,
            ..Paragraph::default()
        };
        blocks.push(Block::Paragraph(std::mem::replace(pending, next)));
    }
}
fn html_inline(
    element: ElementRef<'_>,
    style: &Style,
    directory: &Path,
    paragraph: &mut Paragraph,
    links: HtmlLinks,
) -> Result<()> {
    ensure!(
        element.ancestors().count() <= MAX_HTML_DEPTH,
        "The HTML document is nested too deeply to import safely."
    );
    for node in element.children() {
        if let Some(child) = ElementRef::wrap(node) {
            let mut style = html_style(child, style, paragraph);
            match child.value().name() {
                "script" | "style" | "noscript" => {}
                "br" => push_text(paragraph, "\n", &style),
                "a" => {
                    if let Some(link) = child.value().attr("href") {
                        style.link = links.read(link)?;
                    }
                    html_inline(child, &style, directory, paragraph, links)?;
                }
                "img" => {
                    let source = child
                        .value()
                        .attr("src")
                        .context("An HTML image has no source")?;
                    let bytes = if let Some(data) = source.strip_prefix("data:") {
                        let (mime, encoded) = data
                            .split_once(",")
                            .context("Invalid embedded HTML image")?;
                        ensure!(
                            encoded.len() as u64 <= MAX_PART * 4 / 3 + 8,
                            "An embedded image exceeds the safety limit."
                        );
                        ensure!(
                            mime.ends_with(";base64"),
                            "Only base64-encoded embedded HTML images are supported."
                        );
                        STANDARD.decode(encoded.trim())?
                    } else {
                        ensure!(
                            !source.contains(':') && !source.starts_with('/'),
                            "Remote HTML images cannot be imported offline. Embed the image in the document first."
                        );
                        ensure!(
                            Path::new(source)
                                .components()
                                .all(|component| matches!(component, Component::Normal(_))),
                            "An HTML image points outside its document folder."
                        );
                        let root = fs::canonicalize(directory)?;
                        let target = fs::canonicalize(directory.join(source))?;
                        ensure!(
                            target.starts_with(&root),
                            "An HTML image points outside its document folder."
                        );
                        ensure!(
                            fs::metadata(&target)?.len() <= MAX_PART,
                            "An HTML image exceeds the safety limit."
                        );
                        fs::read(target)?
                    };
                    ensure!(
                        bytes.len() as u64 <= MAX_PART,
                        "An embedded image exceeds the safety limit."
                    );
                    let mut geometry = Paragraph::default();
                    let mut image_style = Style::default();
                    if let Some(css) = child.value().attr("style") {
                        apply_css(css, &mut image_style, &mut geometry);
                    }
                    let dimension = |name: &str| {
                        child.value().attr(name).and_then(length_twips).or_else(|| {
                            child.value().attr("style").and_then(|css| {
                                css.split(';')
                                    .filter_map(|declaration| declaration.split_once(':'))
                                    .find(|(key, _)| key.trim() == name)
                                    .and_then(|(_, value)| length_twips(value))
                            })
                        })
                    };
                    let width = dimension("width").unwrap_or(2880);
                    let height = dimension("height").unwrap_or(2160);
                    let picture = package_image(bytes, source, width, height)?;
                    paragraph.runs.push(Run {
                        text: String::new(),
                        style,
                        picture: Some(picture),
                    });
                }
                "ul" | "ol" | "table" => bail!(
                    "A nested HTML list or table cannot be imported into an inline paragraph without changing its structure."
                ),
                _ => html_inline(child, &style, directory, paragraph, links)?,
            }
        } else if let Some(text) = node.value().as_text() {
            push_text(paragraph, &text.text, style);
        }
    }
    Ok(())
}

fn odt_style(properties: Node<'_, '_>, style: &mut Style, paragraph: &mut Paragraph) {
    for attribute in properties.attributes() {
        let name = attribute.name();
        let value = attribute.value();
        match name {
            "font-name" | "font-family" => style.font = value.trim_matches('\'').into(),
            "font-size" => {
                if let Some(size) = length_twips(value) {
                    style.size = size as f32 / 20.0;
                }
            }
            "font-weight" => style.bold = value == "bold",
            "font-style" => style.italic = value == "italic",
            "text-underline-style" => style.underline = value != "none",
            "text-line-through-style" => style.strike = value != "none",
            "color" => {
                if let Some(color) = color(value) {
                    style.foreground = color;
                }
            }
            "background-color" => style.background = color(value),
            "text-position" => {
                style.position = if value.starts_with("super") {
                    1
                } else if value.starts_with("sub") {
                    -1
                } else {
                    0
                }
            }
            "text-align" => {
                paragraph.alignment = match value {
                    "center" => Alignment::Center,
                    "end" | "right" => Alignment::Right,
                    "justify" => Alignment::Justify,
                    _ => Alignment::Left,
                }
            }
            "margin-left" => paragraph.left = length_twips(value).unwrap_or(0),
            "margin-right" => paragraph.right = length_twips(value).unwrap_or(0),
            "text-indent" => paragraph.first = length_twips(value).unwrap_or(0),
            "margin-top" => paragraph.before = length_twips(value).unwrap_or(0),
            "margin-bottom" => paragraph.after = length_twips(value).unwrap_or(0),
            "line-height" => paragraph.line = length_twips(value).unwrap_or(0),
            _ => {}
        }
    }
}
fn odt_styles(text: &str, styles: &mut BTreeMap<String, (Style, Paragraph)>) -> Result<()> {
    let document = XmlDocument::parse(text)?;
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "style")
    {
        if let Some(name) = value(node, "name") {
            let (mut style, mut paragraph) = value(node, "parent-style-name")
                .and_then(|name| styles.get(name))
                .cloned()
                .unwrap_or_default();
            for properties in node.children().filter(Node::is_element) {
                odt_style(properties, &mut style, &mut paragraph);
            }
            styles.insert(name.into(), (style, paragraph));
        }
    }
    for node in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "list-style")
    {
        if let Some(name) = value(node, "name") {
            let list = if node.children().any(|node| {
                node.is_element() && node.tag_name().name() == "list-level-style-bullet"
            }) {
                List::Bullet
            } else {
                List::Number
            };
            styles.insert(
                name.into(),
                (
                    Style::default(),
                    Paragraph {
                        list,
                        ..Paragraph::default()
                    },
                ),
            );
        }
    }
    Ok(())
}
fn read_odt(bytes: &[u8]) -> Result<Document> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).context("Opening the OpenDocument package")?;
    validate_archive(&mut archive)?;
    let content = String::from_utf8(zip_part(&mut archive, "content.xml")?)?;
    let mut styles = BTreeMap::new();
    let styles_xml = optional_zip_text(&mut archive, "styles.xml")?;
    if let Some(text) = &styles_xml {
        odt_styles(text, &mut styles)?;
    }
    odt_styles(&content, &mut styles)?;
    let xml = XmlDocument::parse(&content)?;
    let text = xml
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "text")
        .context("The OpenDocument package has no text body")?;
    let mut document = Document::default();
    odt_blocks(
        text,
        &styles,
        &mut archive,
        &mut document.blocks,
        List::None,
    )?;
    if let Some(text) = styles_xml {
        let xml = XmlDocument::parse(&text)?;
        if let Some(properties) = xml
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == "page-layout-properties")
        {
            document.width = value(properties, "page-width")
                .and_then(length_twips)
                .unwrap_or(document.width);
            document.height = value(properties, "page-height")
                .and_then(length_twips)
                .unwrap_or(document.height);
            document.margins =
                ["margin-left", "margin-right", "margin-top", "margin-bottom"].map(|name| {
                    value(properties, name)
                        .and_then(length_twips)
                        .unwrap_or(1440)
                });
        }
    }
    Ok(document)
}
fn odt_blocks(
    node: Node<'_, '_>,
    styles: &BTreeMap<String, (Style, Paragraph)>,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    blocks: &mut Vec<Block>,
    list: List,
) -> Result<()> {
    ensure!(
        node.ancestors().count() <= MAX_HTML_DEPTH,
        "The OpenDocument file is nested too deeply to import safely."
    );
    for element in node.children().filter(Node::is_element) {
        match element.tag_name().name() {
            "p" | "h" => {
                let (style, mut paragraph) = value(element, "style-name")
                    .and_then(|name| styles.get(name))
                    .cloned()
                    .unwrap_or_default();
                paragraph.list = list;
                paragraph.runs.clear();
                odt_inline(element, &style, &mut paragraph, styles, archive)?;
                blocks.push(Block::Paragraph(paragraph));
            }
            "list" => {
                let kind = value(element, "style-name")
                    .and_then(|name| styles.get(name))
                    .map(|(_, paragraph)| paragraph.list)
                    .filter(|list| *list != List::None)
                    .unwrap_or(List::Number);
                odt_blocks(element, styles, archive, blocks, kind)?;
            }
            "list-item" | "section" => odt_blocks(element, styles, archive, blocks, list)?,
            "table" => {
                let mut rows = Vec::new();
                for row in element
                    .children()
                    .filter(|node| node.is_element() && node.tag_name().name() == "table-row")
                {
                    ensure!(
                        number(row, "number-rows-repeated", 1) == 1,
                        "Repeated OpenDocument table rows cannot be imported safely."
                    );
                    let mut cells = Vec::new();
                    for cell in row
                        .children()
                        .filter(|node| node.is_element() && node.tag_name().name() == "table-cell")
                    {
                        ensure!(
                            number(cell, "number-columns-spanned", 1) == 1
                                && number(cell, "number-rows-spanned", 1) == 1
                                && number(cell, "number-columns-repeated", 1) == 1,
                            "Merged or repeated OpenDocument cells cannot be imported without changing their layout."
                        );
                        let mut content = Vec::new();
                        odt_blocks(cell, styles, archive, &mut content, List::None)?;
                        let mut paragraphs = Vec::new();
                        for block in content {
                            match block {
                                Block::Paragraph(paragraph) => paragraphs.push(paragraph),
                                _ => bail!("Nested OpenDocument tables cannot be imported safely."),
                            }
                        }
                        cells.push(paragraphs);
                    }
                    rows.push(cells);
                }
                blocks.push(Block::Table(rows));
            }
            "soft-page-break" | "sequence-decls" | "tracked-changes" => {}
            _ => {}
        }
    }
    Ok(())
}
fn odt_inline(
    node: Node<'_, '_>,
    style: &Style,
    paragraph: &mut Paragraph,
    styles: &BTreeMap<String, (Style, Paragraph)>,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
) -> Result<()> {
    ensure!(
        node.ancestors().count() <= MAX_HTML_DEPTH,
        "The OpenDocument file is nested too deeply to import safely."
    );
    for element in node.children() {
        if element.is_text() {
            push_text(paragraph, element.text().unwrap_or(""), style);
            continue;
        }
        if !element.is_element() {
            continue;
        }
        match element.tag_name().name() {
            "span" => {
                let style = value(element, "style-name")
                    .and_then(|name| styles.get(name))
                    .map(|(style, _)| style)
                    .unwrap_or(style);
                odt_inline(element, style, paragraph, styles, archive)?;
            }
            "a" => {
                let mut style = style.clone();
                style.link = Some(safe_link(value(element, "href").unwrap_or(""))?);
                odt_inline(element, &style, paragraph, styles, archive)?;
            }
            "s" => {
                let count = number(element, "c", 1);
                ensure!(
                    (1..=100_000).contains(&count),
                    "An OpenDocument space run exceeds the safety limit."
                );
                push_text(paragraph, &" ".repeat(count as usize), style);
            }
            "tab" => push_text(paragraph, "\t", style),
            "line-break" => push_text(paragraph, "\n", style),
            "frame" => {
                let image = element
                    .children()
                    .find(|node| node.is_element() && node.tag_name().name() == "image")
                    .context("The OpenDocument contains an unsupported drawing or object")?;
                let target =
                    value(image, "href").context("An OpenDocument image has no attachment")?;
                let name = package_target("", target.trim_start_matches("./"))?;
                let width = value(element, "width")
                    .and_then(length_twips)
                    .unwrap_or(2880);
                let height = value(element, "height")
                    .and_then(length_twips)
                    .unwrap_or(2160);
                let picture = package_image(zip_part(archive, &name)?, &name, width, height)?;
                paragraph.runs.push(Run {
                    text: String::new(),
                    style: style.clone(),
                    picture: Some(picture),
                });
            }
            "bookmark" | "bookmark-start" | "bookmark-end" | "soft-page-break" => {}
            "note" | "object" | "object-ole" => bail!(
                "The OpenDocument contains notes or embedded objects that cannot be imported safely."
            ),
            _ => odt_inline(element, style, paragraph, styles, archive)?,
        }
    }
    Ok(())
}

fn write_odt_paragraph(
    paragraph: &Paragraph,
    package: &mut PackageOutput,
    styles: &mut String,
    style_id: &mut usize,
) -> Result<String> {
    *style_id += 1;
    let paragraph_id = *style_id;
    let alignment = match paragraph.alignment {
        Alignment::Left => "start",
        Alignment::Center => "center",
        Alignment::Right => "end",
        Alignment::Justify => "justify",
    };
    styles.push_str(&format!("<style:style style:name=\"P{paragraph_id}\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"{alignment}\" fo:margin-left=\"{}pt\" fo:margin-right=\"{}pt\" fo:text-indent=\"{}pt\" fo:margin-top=\"{}pt\" fo:margin-bottom=\"{}pt\"/></style:style>",paragraph.left as f32/20.0,paragraph.right as f32/20.0,paragraph.first as f32/20.0,paragraph.before as f32/20.0,paragraph.after as f32/20.0));
    let mut output = format!("<text:p text:style-name=\"P{paragraph_id}\">");
    for run in &paragraph.runs {
        *style_id += 1;
        let id = *style_id;
        let style = &run.style;
        styles.push_str(&format!("<style:style style:name=\"S{id}\" style:family=\"text\"><style:text-properties fo:font-family=\"{}\" fo:font-size=\"{}pt\" fo:font-weight=\"{}\" fo:font-style=\"{}\" fo:color=\"#{:06x}\" style:text-underline-style=\"{}\" style:text-line-through-style=\"{}\"",xml(&style.font),style.size,if style.bold{"bold"}else{"normal"},if style.italic{"italic"}else{"normal"},style.foreground,if style.underline{"solid"}else{"none"},if style.strike{"solid"}else{"none"}));
        if let Some(color) = style.background {
            styles.push_str(&format!(" fo:background-color=\"#{color:06x}\""));
        }
        if style.position != 0 {
            styles.push_str(&format!(
                " style:text-position=\"{} 58%\"",
                if style.position > 0 { "super" } else { "sub" }
            ));
        }
        styles.push_str("/></style:style>");
        if let Some(link) = &style.link {
            output.push_str(&format!(
                "<text:a xlink:type=\"simple\" xlink:href=\"{}\">",
                xml(link)
            ));
        }
        output.push_str(&format!("<text:span text:style-name=\"S{id}\">"));
        if let Some(picture) = &run.picture {
            package.pictures.push(picture.clone());
            let image_id = package.pictures.len();
            output.push_str(&format!("<draw:frame draw:name=\"Picture{image_id}\" text:anchor-type=\"as-char\" svg:width=\"{}pt\" svg:height=\"{}pt\"><draw:image xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\" xlink:href=\"Pictures/image{image_id}.{}\"/></draw:frame>",picture.width as f32/20.0,picture.height as f32/20.0,picture_extension(picture)?));
        } else {
            for character in run.text.chars() {
                match character {
                    ' ' => output.push_str("<text:s/>"),
                    '\t' => output.push_str("<text:tab/>"),
                    '\n' => output.push_str("<text:line-break/>"),
                    _ => output.push_str(&xml(&character.to_string())),
                }
            }
        }
        output.push_str("</text:span>");
        if style.link.is_some() {
            output.push_str("</text:a>");
        }
    }
    output.push_str("</text:p>");
    if paragraph.list != List::None {
        output = format!(
            "<text:list text:style-name=\"{}\" text:continue-numbering=\"true\"><text:list-item>{output}</text:list-item></text:list>",
            if paragraph.list == List::Bullet {
                "ExBullet"
            } else {
                "ExNumber"
            }
        );
    }
    Ok(output)
}
fn write_odt(document: &Document) -> Result<Vec<u8>> {
    let namespaces = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"";
    let mut styles = String::from(
        "<text:list-style style:name=\"ExBullet\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"•\"/></text:list-style><text:list-style style:name=\"ExNumber\"><text:list-level-style-number text:level=\"1\" style:num-format=\"1\" style:num-suffix=\".\"/></text:list-style>",
    );
    let mut body = String::new();
    let mut package = PackageOutput::default();
    let mut style_id = 0;
    for block in &document.blocks {
        match block {
            Block::Paragraph(paragraph) => body.push_str(&write_odt_paragraph(
                paragraph,
                &mut package,
                &mut styles,
                &mut style_id,
            )?),
            Block::Table(rows) => {
                body.push_str("<table:table>");
                for row in rows {
                    body.push_str("<table:table-row>");
                    for cell in row {
                        body.push_str("<table:table-cell>");
                        for paragraph in cell {
                            body.push_str(&write_odt_paragraph(
                                paragraph,
                                &mut package,
                                &mut styles,
                                &mut style_id,
                            )?);
                        }
                        if cell.is_empty() {
                            body.push_str("<text:p/>");
                        }
                        body.push_str("</table:table-cell>");
                    }
                    body.push_str("</table:table-row>");
                }
                body.push_str("</table:table>");
            }
        }
    }
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {namespaces} office:version=\"1.3\"><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"
    );
    let page_styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {namespaces} office:version=\"1.3\"><office:styles/><office:automatic-styles><style:page-layout style:name=\"Page\"><style:page-layout-properties fo:page-width=\"{}pt\" fo:page-height=\"{}pt\" fo:margin-left=\"{}pt\" fo:margin-right=\"{}pt\" fo:margin-top=\"{}pt\" fo:margin-bottom=\"{}pt\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"Page\"/></office:master-styles></office:document-styles>",
        document.width as f32 / 20.0,
        document.height as f32 / 20.0,
        document.margins[0] as f32 / 20.0,
        document.margins[1] as f32 / 20.0,
        document.margins[2] as f32 / 20.0,
        document.margins[3] as f32 / 20.0
    );
    let mut manifest = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>",
    );
    for (index, picture) in package.pictures.iter().enumerate() {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"Pictures/image{}.{}\" manifest:media-type=\"{}\"/>",index+1,picture_extension(picture)?,picture.mime));
    }
    manifest.push_str("</manifest:manifest>");
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    add_zip(
        &mut writer,
        "mimetype",
        b"application/vnd.oasis.opendocument.text",
        true,
    )?;
    add_zip(&mut writer, "content.xml", content.as_bytes(), false)?;
    add_zip(&mut writer, "styles.xml", page_styles.as_bytes(), false)?;
    add_zip(
        &mut writer,
        "META-INF/manifest.xml",
        manifest.as_bytes(),
        false,
    )?;
    for (index, picture) in package.pictures.iter().enumerate() {
        add_zip(
            &mut writer,
            &format!(
                "Pictures/image{}.{}",
                index + 1,
                picture_extension(picture)?
            ),
            &picture.bytes,
            false,
        )?;
    }
    Ok(writer.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn markdown_preview_keeps_relative_and_unsupported_link_labels() {
        let source = "# Linked document\n\n[Relative](notes/next.md) and [Parent](../README.md).\n\n[Local](file:///C:/notes/example.md) and [Custom](obsidian://open?vault=notes).\n\n<a href=\"javascript:alert(1)\">Raw HTML label</a> and [Script](javascript:alert%281%29).\n\n[Web](HTTPS://example.com/page) and [Anchor](#linked-document).\n\nAfter the links, **bold text** and emoji 😀 remain.";
        let rtf = markdown_to_rtf(source, Path::new(".")).expect("links do not abort preview");
        let document = read_rtf(&rtf).expect("valid preview RTF");
        let runs: Vec<_> = document
            .blocks
            .iter()
            .flat_map(|block| match block {
                Block::Paragraph(paragraph) => paragraph.runs.iter().collect::<Vec<_>>(),
                Block::Table(_) => Vec::new(),
            })
            .collect();
        let text: String = runs.iter().map(|run| run.text.as_str()).collect();
        for label in [
            "Relative",
            "Parent",
            "Local",
            "Custom",
            "Raw HTML label",
            "Script",
            "After the links",
            "😀",
        ] {
            assert!(text.contains(label), "missing {label}");
        }
        let links: Vec<_> = runs
            .iter()
            .filter_map(|run| run.style.link.as_deref())
            .collect();
        assert_eq!(links, ["HTTPS://example.com/page", "#linked-document"]);
        assert!(
            runs.iter()
                .any(|run| run.style.bold && run.text.contains("bold text"))
        );
    }
    #[test]
    fn unsupported_links_remain_rejected_by_strict_document_import() {
        for link in [
            "javascript:alert(1)",
            "file:///C:/app.exe",
            "shell:AppsFolder",
            "data:text/html,test",
            "notes/file.md",
            "https://example.com/\nnext",
            "https://example.com/\"quoted",
        ] {
            assert!(safe_link(link).is_err(), "unsafe link {link:?}");
        }
        for link in [
            "https://example.com",
            "HTTP://example.com",
            "MAILTO:user@example.com",
            "tel:+123",
            "#section",
        ] {
            assert_eq!(safe_link(link).expect("allowed link"), link);
        }
        assert!(read_html(b"<a href=\"javascript:alert(1)\">label</a>", Path::new(".")).is_err());
    }
    #[test]
    fn markdown_renders_nested_lists_tables_and_tasks() {
        let source = "# Title

Some **bold** and `code`.

- outer
  - inner

| a | b |
|---|---|
| 1 | 2 |

- [x] done

![remote](https://example.com/x.png)
";
        let rtf = String::from_utf8(markdown_to_rtf(source, Path::new(".")).expect("renders"))
            .expect("RTF is ASCII");
        for expected in ["Title", "bold", "outer", "inner", "done", "remote"] {
            assert!(rtf.contains(expected), "{expected} missing");
        }
        assert!(rtf.contains(r"\b"), "bold run");
        assert!(rtf.contains(r"\li360"), "nested item is indented");
        assert!(
            !rtf.contains(r"舦?}} "),
            "no stray space starts a list item"
        );
        assert!(
            !rtf.contains(r"outer\line"),
            "no blank line before the nested list"
        );
        assert!(
            rtf.contains(crate::fonts::RICH_FONT),
            "body uses the rich font"
        );
    }
    fn fixture() -> Document {
        let mut paragraph = Paragraph {
            alignment: Alignment::Center,
            ..Paragraph::default()
        };
        let style = Style {
            font: "Helvetica".into(),
            size: 14.0,
            bold: true,
            italic: true,
            foreground: 0x345678,
            ..Style::default()
        };
        push_text(&mut paragraph, "Hello 🦀 & <world>\nSecond\tline", &style);
        Document {
            blocks: vec![Block::Paragraph(paragraph)],
            ..Document::default()
        }
    }
    fn assert_fixture(document: &Document) {
        let Block::Paragraph(paragraph) = &document.blocks[0] else {
            panic!("expected a paragraph")
        };
        assert_eq!(paragraph.alignment, Alignment::Center);
        let text = paragraph
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        assert_eq!(text, "Hello 🦀 & <world>\nSecond\tline");
        assert!(paragraph.runs[0].style.bold);
        assert!(paragraph.runs[0].style.italic);
        assert_eq!(paragraph.runs[0].style.foreground, 0x345678);
    }
    #[test]
    fn rtf_round_trip_preserves_unicode_and_inline_styles() {
        let encoded = write_rtf(&fixture()).expect("RTF fixture");
        assert_fixture(&read_rtf(&encoded).expect("RTF round trip"));
    }
    #[test]
    fn docx_round_trip_preserves_unicode_and_inline_styles() {
        let encoded = write_docx(&fixture()).expect("Word fixture");
        assert_fixture(&read_docx(&encoded).expect("Word round trip"));
    }
    #[test]
    fn odt_round_trip_preserves_unicode_and_inline_styles() {
        let encoded = write_odt(&fixture()).expect("OpenDocument fixture");
        assert_fixture(&read_odt(&encoded).expect("OpenDocument round trip"));
    }
    #[test]
    fn html_round_trip_preserves_unicode_and_inline_styles() {
        let encoded = write_html(&fixture()).expect("HTML fixture");
        let decoded = read_html(encoded.as_bytes(), Path::new(".")).expect("HTML round trip");
        assert_fixture(&decoded);
    }
    #[test]
    fn embedded_objects_fail_before_export() {
        let result = export(br"{\rtf1 {\object unsupported}}", DocumentFormat::Docx);
        assert!(result.is_err());
    }
    #[test]
    fn package_paths_cannot_escape_the_archive() {
        assert!(package_target("word", "../../outside").is_err());
        assert!(package_target("word", "https://example.com/image").is_err());
        assert_eq!(
            package_target("word", "media/image.png").expect("local attachment"),
            "word/media/image.png"
        );
    }
    #[test]
    fn malformed_unicode_and_unbalanced_rtf_are_rejected() {
        assert!(read_rtf(br"{\rtf1\uc1\u-10178?}").is_err());
        assert!(read_rtf(br"{\rtf1 unclosed").is_err());
    }
}
