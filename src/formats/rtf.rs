//! Small RTF codec with scoped character state and bounded tokenization.
use super::{
    MAX_DEPTH, MAX_HTML_BYTES, MAX_IMAGE_BYTES, PageSettings, RichDocument, escape, image_data,
    image_uri, is_safe_href, safe_native_html,
};
use anyhow::{Context, Result, bail, ensure};
use scraper::ElementRef;
use std::{collections::BTreeMap, fmt::Write as _, io::Cursor};

#[derive(Debug)]
enum Token {
    Group(Vec<Token>),
    Control(String, Option<i32>),
    Text(Vec<u8>),
    Binary(Vec<u8>),
}
fn push_text(tokens: &mut Vec<Token>, bytes: Vec<u8>) {
    if let Some(Token::Text(previous)) = tokens.last_mut() {
        previous.extend(bytes);
    } else {
        tokens.push(Token::Text(bytes));
    }
}
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    count: usize,
}
impl Parser<'_> {
    fn group(&mut self, depth: usize) -> Result<Vec<Token>> {
        ensure!(depth <= MAX_DEPTH, "RTF is nested too deeply.");
        let mut tokens = Vec::new();
        while self.pos < self.bytes.len() {
            self.count += 1;
            ensure!(self.count <= 2_000_000, "RTF contains too many tokens.");
            let byte = self.bytes[self.pos];
            self.pos += 1;
            match byte {
                b'{' => tokens.push(Token::Group(self.group(depth + 1)?)),
                b'}' => return Ok(tokens),
                b'\\' => {
                    let next = *self.bytes.get(self.pos).context("Truncated RTF escape")?;
                    self.pos += 1;
                    match next {
                        b'\\' | b'{' | b'}' => push_text(&mut tokens, vec![next]),
                        b'\'' => {
                            let hex = self
                                .bytes
                                .get(self.pos..self.pos + 2)
                                .context("Truncated RTF hex escape")?;
                            let text = std::str::from_utf8(hex)?;
                            push_text(
                                &mut tokens,
                                vec![
                                    u8::from_str_radix(text, 16)
                                        .context("Invalid RTF hex escape")?,
                                ],
                            );
                            self.pos += 2;
                        }
                        b'\r' | b'\n' => {}
                        b'a'..=b'z' | b'A'..=b'Z' => {
                            let start = self.pos - 1;
                            while self
                                .bytes
                                .get(self.pos)
                                .is_some_and(u8::is_ascii_alphabetic)
                            {
                                self.pos += 1;
                            }
                            ensure!(self.pos - start <= 64, "RTF control word is too long.");
                            let word =
                                std::str::from_utf8(&self.bytes[start..self.pos])?.to_owned();
                            let start = self.pos;
                            if self.bytes.get(self.pos) == Some(&b'-') {
                                self.pos += 1;
                            }
                            while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
                                self.pos += 1;
                            }
                            let number = if self.pos > start {
                                Some(
                                    std::str::from_utf8(&self.bytes[start..self.pos])?
                                        .parse::<i32>()
                                        .context("Invalid RTF numeric parameter")?,
                                )
                            } else {
                                None
                            };
                            if self.bytes.get(self.pos) == Some(&b' ') {
                                self.pos += 1;
                            }
                            if word == "bin" {
                                let size = usize::try_from(
                                    number.context("RTF binary data has no length")?,
                                )?;
                                ensure!(
                                    size as u64 <= MAX_IMAGE_BYTES,
                                    "RTF binary resource exceeds its limit."
                                );
                                let end = self
                                    .pos
                                    .checked_add(size)
                                    .context("RTF binary length overflow")?;
                                tokens.push(Token::Binary(
                                    self.bytes
                                        .get(self.pos..end)
                                        .context("Truncated RTF binary data")?
                                        .to_vec(),
                                ));
                                self.pos = end;
                            } else {
                                tokens.push(Token::Control(word, number));
                            }
                        }
                        _ => tokens.push(Token::Control(char::from(next).to_string(), None)),
                    }
                }
                b'\r' | b'\n' => {}
                _ => {
                    let start = self.pos - 1;
                    while self
                        .bytes
                        .get(self.pos)
                        .is_some_and(|b| !matches!(*b, b'{' | b'}' | b'\\' | b'\r' | b'\n'))
                    {
                        self.pos += 1;
                    }
                    push_text(&mut tokens, self.bytes[start..self.pos].to_vec());
                }
            }
        }
        bail!("Unclosed RTF group")
    }
}
#[derive(Clone, Default)]
struct State {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    vertical: i8,
    font: i32,
    size: i32,
    color: i32,
    background: i32,
    align: i32,
    indent: i32,
    first: i32,
    uc: usize,
    in_table: bool,
    link: Option<String>,
}
impl State {
    fn character_style(&self, fonts: &BTreeMap<i32, String>, colors: &[String]) -> String {
        let mut style = String::new();
        if self.bold {
            style.push_str("font-weight:bold;");
        }
        if self.italic {
            style.push_str("font-style:italic;");
        }
        if self.underline {
            style.push_str("text-decoration:underline;");
        } else if self.strike {
            style.push_str("text-decoration:line-through;");
        }
        if let Some(font) = fonts.get(&self.font) {
            let _ = write!(
                style,
                "font-family:{};",
                font.replace(['\'', '"', ';', ':'], "")
            );
        }
        if (2..=3276).contains(&self.size) {
            let _ = write!(style, "font-size:{}pt;", self.size as f32 / 2.);
        }
        for (key, index) in [("color", self.color), ("background-color", self.background)] {
            if let Some(value) = usize::try_from(index)
                .ok()
                .and_then(|i| colors.get(i))
                .filter(|v| !v.is_empty())
            {
                let _ = write!(style, "{key}:{value};");
            }
        }
        style
    }
}
#[derive(Default)]
struct Output {
    blocks: String,
    paragraph: String,
    cell: String,
    cells: String,
    rows: String,
    row: bool,
    high: Option<u16>,
}
impl Output {
    fn text(
        &mut self,
        text: &str,
        state: &State,
        fonts: &BTreeMap<i32, String>,
        colors: &[String],
    ) {
        if !state.in_table && !self.row {
            self.table();
        }
        if text.is_empty() {
            return;
        }
        let style = state.character_style(fonts, colors);
        let mut text = escape(text);
        if !style.is_empty() {
            text = format!("<span style=\"{}\">{text}</span>", escape(&style));
        }
        if state.vertical != 0 {
            let tag = if state.vertical > 0 { "sup" } else { "sub" };
            text = format!("<{tag}>{text}</{tag}>");
        }
        if let Some(href) = &state.link {
            text = format!("<a href=\"{}\">{text}</a>", escape(href));
        }
        self.paragraph.push_str(&text);
    }
    fn paragraph(&mut self, state: &State) {
        let mut style = String::new();
        if let Some(align) = ["", "right", "center", "justify"]
            .get(state.align as usize)
            .filter(|v| !v.is_empty())
        {
            let _ = write!(style, "text-align:{align};");
        }
        if state.indent != 0 {
            let _ = write!(style, "margin-left:{}pt;", state.indent as f32 / 20.);
        }
        if state.first != 0 {
            let _ = write!(style, "text-indent:{}pt;", state.first as f32 / 20.);
        }
        let markup = format!(
            "<p style=\"{}\">{}</p>",
            escape(&style),
            std::mem::take(&mut self.paragraph)
        );
        if self.row || state.in_table {
            self.cell.push_str(&markup);
        } else {
            self.table();
            self.blocks.push_str(&markup);
        }
    }
    fn cell(&mut self, state: &State) {
        if !self.paragraph.is_empty() {
            self.paragraph(state);
        }
        self.cells.push_str("<td>");
        self.cells.push_str(&std::mem::take(&mut self.cell));
        self.cells.push_str("</td>");
    }
    fn row(&mut self, state: &State) {
        if !self.paragraph.is_empty() || !self.cell.is_empty() {
            self.cell(state);
        }
        self.rows.push_str("<tr>");
        self.rows.push_str(&std::mem::take(&mut self.cells));
        self.rows.push_str("</tr>");
        self.row = false;
    }
    fn table(&mut self) {
        if !self.rows.is_empty() {
            self.blocks.push_str("<table>");
            self.blocks.push_str(&std::mem::take(&mut self.rows));
            self.blocks.push_str("</table>");
        }
    }
}
fn plain_tokens(tokens: &[Token], out: &mut Vec<u8>) {
    for token in tokens {
        match token {
            Token::Text(bytes) => out.extend_from_slice(bytes),
            Token::Group(group) => plain_tokens(group, out),
            _ => {}
        }
    }
}
fn control(tokens: &[Token], name: &str) -> Option<i32> {
    tokens.iter().find_map(|t| match t {
        Token::Control(word, value) if word == name => *value,
        _ => None,
    })
}
fn destination(tokens: &[Token]) -> Option<&str> {
    tokens.iter().find_map(|t| match t {
        Token::Control(word, _) if word != "*" => Some(word.as_str()),
        Token::Text(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => None,
        _ => None,
    })
}
fn metadata(tokens: &[Token], fonts: &mut BTreeMap<i32, String>, colors: &mut Vec<String>) {
    for token in tokens {
        if let Token::Group(group) = token {
            match destination(group) {
                Some("fonttbl") => {
                    for token in group {
                        if let Token::Group(font) = token
                            && let Some(index) = control(font, "f")
                        {
                            let mut text = Vec::new();
                            plain_tokens(font, &mut text);
                            let text = encoding_rs::WINDOWS_1252
                                .decode(&text)
                                .0
                                .trim()
                                .trim_end_matches(';')
                                .to_owned();
                            fonts.insert(index, text);
                        }
                    }
                }
                Some("colortbl") => {
                    let (mut red, mut green, mut blue, mut set) = (0, 0, 0, false);
                    for token in group {
                        match token {
                            Token::Control(word, Some(value)) => match word.as_str() {
                                "red" => {
                                    red = *value;
                                    set = true;
                                }
                                "green" => {
                                    green = *value;
                                    set = true;
                                }
                                "blue" => {
                                    blue = *value;
                                    set = true;
                                }
                                _ => {}
                            },
                            Token::Text(bytes) => {
                                for byte in bytes {
                                    if *byte == b';' {
                                        colors.push(if set {
                                            format!(
                                                "#{:02x}{:02x}{:02x}",
                                                red.clamp(0, 255),
                                                green.clamp(0, 255),
                                                blue.clamp(0, 255)
                                            )
                                        } else {
                                            String::new()
                                        });
                                        (red, green, blue, set) = (0, 0, 0, false);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => metadata(group, fonts, colors),
            }
        }
    }
}
struct Renderer {
    output: Output,
    fonts: BTreeMap<i32, String>,
    colors: Vec<String>,
    encoding: &'static encoding_rs::Encoding,
    skip: usize,
    page: PageSettings,
}
impl Renderer {
    fn render(&mut self, tokens: &[Token], mut state: State) -> Result<()> {
        for token in tokens {
            match token {
                Token::Group(group) => {
                    let dest = destination(group).unwrap_or("");
                    if dest == "field" {
                        let instruction = group.iter().find_map(|t| match t {
                            Token::Group(g) if destination(g) == Some("fldinst") => Some(g),
                            _ => None,
                        });
                        let mut field_state = state.clone();
                        if let Some(instruction) = instruction {
                            let mut raw = Vec::new();
                            plain_tokens(instruction, &mut raw);
                            let raw = self.encoding.decode(&raw).0;
                            let raw = raw.trim();
                            if raw
                                .get(..9)
                                .is_some_and(|s| s.eq_ignore_ascii_case("HYPERLINK"))
                            {
                                let target = raw[9..].trim();
                                let target = target
                                    .strip_prefix('"')
                                    .and_then(|s| s.split_once('"').map(|v| v.0))
                                    .unwrap_or_else(|| {
                                        target.split_whitespace().next().unwrap_or("")
                                    });
                                if is_safe_href(target) {
                                    field_state.link = Some(target.to_owned());
                                }
                            }
                        }
                        for child in group {
                            if let Token::Group(g) = child
                                && destination(g) == Some("fldrslt")
                            {
                                self.render(g, field_state.clone())?;
                            }
                        }
                    } else if dest == "pict" {
                        let mut bytes = Vec::new();
                        let mut hex = Vec::new();
                        for item in group {
                            match item {
                                Token::Binary(data) => bytes.extend_from_slice(data),
                                Token::Text(text) => hex.extend(
                                    text.iter().copied().filter(|b| !b.is_ascii_whitespace()),
                                ),
                                _ => {}
                            }
                        }
                        ensure!(
                            hex.len() as u64 <= MAX_IMAGE_BYTES * 2,
                            "RTF image exceeds its resource limit."
                        );
                        ensure!(
                            hex.len().is_multiple_of(2),
                            "RTF image has an incomplete hex byte."
                        );
                        for pair in hex.as_chunks::<2>().0 {
                            bytes.push(
                                u8::from_str_radix(std::str::from_utf8(pair)?, 16)
                                    .context("Invalid RTF image hex")?,
                            );
                        }
                        if let Some(uri) = image_uri(&bytes) {
                            let _ = write!(self.output.paragraph, "<img src=\"{uri}\">");
                        }
                    } else if !matches!(
                        dest,
                        "fonttbl"
                            | "colortbl"
                            | "stylesheet"
                            | "info"
                            | "listtable"
                            | "listoverridetable"
                            | "header"
                            | "headerl"
                            | "headerr"
                            | "footer"
                            | "footerl"
                            | "footerr"
                            | "footnote"
                            | "annotation"
                            | "object"
                            | "objdata"
                            | "datastore"
                            | "xmlnstbl"
                            | "themedata"
                            | "colorschememapping"
                            | "fldinst"
                            | "shp"
                            | "nonshppict"
                    ) && (dest == "shppict"
                        || !matches!(group.first(), Some(Token::Control(word, _)) if word == "*"))
                    {
                        self.render(group, state.clone())?;
                    }
                }
                Token::Text(bytes) => {
                    let skip = self.skip.min(bytes.len());
                    self.skip -= skip;
                    let text = self.encoding.decode(&bytes[skip..]).0;
                    if !text.is_empty() {
                        if self.output.high.take().is_some() {
                            self.output.text("�", &state, &self.fonts, &self.colors);
                        }
                        self.output.text(&text, &state, &self.fonts, &self.colors);
                    }
                }
                Token::Control(word, parameter) => {
                    let value = parameter.unwrap_or(1);
                    match word.as_str() {
                        "b" => state.bold = value != 0,
                        "i" => state.italic = value != 0,
                        "ul" => state.underline = value != 0,
                        "ulnone" => state.underline = false,
                        "strike" => state.strike = value != 0,
                        "super" => state.vertical = 1,
                        "sub" => state.vertical = -1,
                        "nosupersub" => state.vertical = 0,
                        "f" => state.font = value,
                        "fs" => state.size = value,
                        "cf" => state.color = value,
                        "highlight" | "cb" => state.background = value,
                        "plain" => {
                            let uc = state.uc;
                            let table = state.in_table;
                            state = State {
                                uc,
                                in_table: table,
                                ..Default::default()
                            };
                        }
                        "pard" => {
                            state.align = 0;
                            state.indent = 0;
                            state.first = 0;
                            state.in_table = false;
                        }
                        "ql" => state.align = 0,
                        "qr" => state.align = 1,
                        "qc" => state.align = 2,
                        "qj" => state.align = 3,
                        "li" => state.indent = value,
                        "fi" => state.first = value,
                        "par" | "sect" => self.output.paragraph(&state),
                        "line" => self.output.paragraph.push_str("<br>"),
                        "tab" => self.output.text("\t", &state, &self.fonts, &self.colors),
                        "trowd" => {
                            if !self.output.paragraph.is_empty() {
                                self.output.paragraph(&state);
                            }
                            self.output.row = true;
                            state.in_table = true;
                        }
                        "intbl" => state.in_table = value != 0,
                        "cell" => self.output.cell(&state),
                        "row" => self.output.row(&state),
                        "uc" => {
                            ensure!(
                                (0..=32).contains(&value),
                                "Invalid RTF Unicode fallback length."
                            );
                            state.uc = value as usize;
                        }
                        "u" => {
                            let unit = value as u16;
                            if (0xd800..=0xdbff).contains(&unit) {
                                self.output.high = Some(unit);
                            } else {
                                let units = self
                                    .output
                                    .high
                                    .take()
                                    .map(|high| vec![high, unit])
                                    .unwrap_or_else(|| vec![unit]);
                                self.output.text(
                                    &String::from_utf16_lossy(&units),
                                    &state,
                                    &self.fonts,
                                    &self.colors,
                                );
                            }
                            self.skip = state.uc;
                        }
                        "ansicpg" => {
                            let label = format!("windows-{value}");
                            self.encoding = encoding_rs::Encoding::for_label(label.as_bytes())
                                .unwrap_or(encoding_rs::WINDOWS_1252);
                        }
                        "~" | "_" | "-" | "emdash" | "endash" | "bullet" | "lquote" | "rquote"
                        | "ldblquote" | "rdblquote" => {
                            if self.skip > 0 {
                                self.skip -= 1;
                            } else {
                                let text = match word.as_str() {
                                    "~" => "\u{a0}",
                                    "_" => "‑",
                                    "-" => "\u{ad}",
                                    "emdash" => "—",
                                    "endash" => "–",
                                    "bullet" => "•",
                                    "lquote" => "‘",
                                    "rquote" => "’",
                                    "ldblquote" => "“",
                                    _ => "”",
                                };
                                self.output.text(text, &state, &self.fonts, &self.colors);
                            }
                        }
                        "paperw" => self.page.width_twips = value,
                        "paperh" => self.page.height_twips = value,
                        "margl" => self.page.margin_left = value,
                        "margr" => self.page.margin_right = value,
                        "margt" => self.page.margin_top = value,
                        "margb" => self.page.margin_bottom = value,
                        _ => {}
                    }
                }
                Token::Binary(_) => {}
            }
            ensure!(
                self.output.blocks.len()
                    + self.output.paragraph.len()
                    + self.output.rows.len()
                    + self.output.cells.len()
                    + self.output.cell.len()
                    <= MAX_HTML_BYTES,
                "RTF expands beyond the document limit."
            );
        }
        Ok(())
    }
}
pub(super) fn read(bytes: &[u8]) -> Result<RichDocument> {
    ensure!(
        bytes.len() <= MAX_HTML_BYTES,
        "The RTF document exceeds its size limit."
    );
    let bytes = bytes.trim_ascii_start();
    ensure!(
        bytes.starts_with(b"{\\rtf"),
        "The document is not valid RTF."
    );
    let mut parser = Parser {
        bytes,
        pos: 1,
        count: 0,
    };
    let tokens = parser.group(1)?;
    ensure!(
        bytes[parser.pos..].iter().all(u8::is_ascii_whitespace),
        "RTF has data after its root group."
    );
    let mut renderer = Renderer {
        output: Output::default(),
        fonts: BTreeMap::new(),
        colors: Vec::new(),
        encoding: encoding_rs::WINDOWS_1252,
        skip: 0,
        page: PageSettings::default(),
    };
    metadata(&tokens, &mut renderer.fonts, &mut renderer.colors);
    renderer.render(
        &tokens,
        State {
            uc: 1,
            ..Default::default()
        },
    )?;
    let state = State::default();
    if renderer.output.row {
        renderer.output.row(&state);
    }
    if !renderer.output.paragraph.is_empty() {
        renderer.output.paragraph(&state);
    }
    renderer.output.table();
    Ok(RichDocument {
        html: safe_native_html(&renderer.output.blocks)?,
        page: renderer.page,
    })
}

fn encoded(text: &str, out: &mut String) {
    for unit in text.encode_utf16() {
        match unit {
            0x5c | 0x7b | 0x7d => {
                out.push('\\');
                out.push(char::from_u32(u32::from(unit)).unwrap_or('�'));
            }
            10 => out.push_str("\\line "),
            13 => {}
            9 => out.push_str("\\tab "),
            32..=126 => out.push(char::from_u32(u32::from(unit)).unwrap_or('�')),
            _ => {
                let _ = write!(out, "\\u{}?", unit as i16);
            }
        }
    }
}
fn style_value<'a>(element: &ElementRef<'a>, key: &str) -> Option<String> {
    element.value().attr("style")?.split(';').find_map(|v| {
        let (name, value) = v.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(key)
            .then(|| value.trim().to_owned())
    })
}
struct Writer {
    fonts: BTreeMap<String, usize>,
    colors: BTreeMap<String, usize>,
}
impl Writer {
    fn children(&self, element: ElementRef<'_>, table: bool, out: &mut String) -> Result<()> {
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                encoded(text, out);
                continue;
            }
            let Some(element) = ElementRef::wrap(child) else {
                continue;
            };
            let tag = element.value().name();
            if tag == "table" {
                for row in element
                    .descendants()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| e.value().name() == "tr")
                {
                    let cells: Vec<_> = row
                        .children()
                        .filter_map(ElementRef::wrap)
                        .filter(|e| matches!(e.value().name(), "td" | "th"))
                        .collect();
                    out.push_str("\\trowd\\trgaph108");
                    for i in 1..=cells.len() {
                        let _ = write!(out, "\\cellx{}", i * 2500);
                    }
                    for cell in cells {
                        out.push_str("\\intbl ");
                        self.children(cell, true, out)?;
                        out.push_str("\\cell ");
                    }
                    out.push_str("\\row\n");
                }
                continue;
            }
            if tag == "img" {
                if let Some(bytes) = element.value().attr("src").and_then(image_data) {
                    let (kind, bytes) = if bytes.starts_with(b"\x89PNG") {
                        ("pngblip", bytes)
                    } else if bytes.starts_with(b"\xff\xd8") {
                        ("jpegblip", bytes)
                    } else {
                        let image = image::load_from_memory(&bytes)?;
                        let mut cursor = Cursor::new(Vec::new());
                        image.write_to(&mut cursor, image::ImageFormat::Png)?;
                        ("pngblip", cursor.into_inner())
                    };
                    let _ = write!(out, "{{\\pict\\{kind} ");
                    for byte in bytes {
                        let _ = write!(out, "{byte:02x}");
                    }
                    out.push('}');
                }
                continue;
            }
            if tag == "br" {
                out.push_str("\\line ");
                continue;
            }
            if tag == "hr" {
                out.push_str("\\par ");
                continue;
            }
            let block = matches!(
                tag,
                "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "li" | "pre"
            );
            out.push('{');
            if block {
                out.push_str("\\pard");
                if table {
                    out.push_str("\\intbl");
                }
            }
            match tag {
                "b" | "strong" => out.push_str("\\b"),
                "i" | "em" => out.push_str("\\i"),
                "u" => out.push_str("\\ul"),
                "s" | "strike" | "del" => out.push_str("\\strike"),
                "sup" => out.push_str("\\super"),
                "sub" => out.push_str("\\sub"),
                "h1" => out.push_str("\\b\\fs48"),
                "h2" => out.push_str("\\b\\fs36"),
                "h3" => out.push_str("\\b\\fs28"),
                _ => {}
            }
            if let Some(style) = element.value().attr("style") {
                for declaration in style.split(';') {
                    let Some((key, value)) = declaration.split_once(':') else {
                        continue;
                    };
                    let value = value.trim();
                    match key.trim() {
                        "font-weight"
                            if value == "bold" || value.parse::<i32>().is_ok_and(|v| v >= 600) =>
                        {
                            out.push_str("\\b")
                        }
                        "font-style" if value == "italic" => out.push_str("\\i"),
                        "text-decoration" if value.contains("underline") => out.push_str("\\ul"),
                        "text-decoration" if value.contains("line-through") => {
                            out.push_str("\\strike")
                        }
                        "font-size" => {
                            let number = value
                                .trim_end_matches("pt")
                                .trim_end_matches("px")
                                .parse::<f32>()
                                .unwrap_or(12.);
                            if number.is_finite() {
                                let _ = write!(
                                    out,
                                    "\\fs{}",
                                    (number * 2.).round().clamp(2., 3276.) as i32
                                );
                            }
                        }
                        "font-family" => {
                            if let Some(index) = self.fonts.get(value) {
                                let _ = write!(out, "\\f{index}");
                            }
                        }
                        "color" | "background-color" => {
                            if let Some(index) = self.colors.get(value) {
                                let word = if key.trim() == "color" {
                                    "cf"
                                } else {
                                    "highlight"
                                };
                                let _ = write!(out, "\\{word}{index}");
                            }
                        }
                        "text-align" => out.push_str(match value {
                            "right" => "\\qr",
                            "center" => "\\qc",
                            "justify" => "\\qj",
                            _ => "\\ql",
                        }),
                        "margin-left" | "text-indent" => {
                            if let Ok(value) = value
                                .trim_end_matches("pt")
                                .trim_end_matches("px")
                                .parse::<f32>()
                                && value.is_finite()
                            {
                                let word = if key.trim() == "margin-left" {
                                    "li"
                                } else {
                                    "fi"
                                };
                                let _ = write!(
                                    out,
                                    "\\{word}{}",
                                    (value * 20.).round().clamp(-28800., 28800.) as i32
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            out.push(' ');
            if tag == "li" {
                encoded("• ", out);
            }
            if tag == "a" {
                if let Some(href) = element.value().attr("href").filter(|h| is_safe_href(h)) {
                    out.push_str("{\\field{\\*\\fldinst HYPERLINK \"");
                    encoded(&href.replace('"', "%22"), out);
                    out.push_str("\"}{\\fldrslt ");
                    self.children(element, table, out)?;
                    out.push_str("}}");
                } else {
                    self.children(element, table, out)?;
                }
            } else {
                self.children(element, table, out)?;
            }
            if block {
                out.push_str("\\par");
            }
            out.push('}');
        }
        Ok(())
    }
}
pub(super) fn write(document: &RichDocument) -> Result<Vec<u8>> {
    let html = safe_native_html(&document.html)?;
    let parsed = scraper::Html::parse_fragment(&html);
    let mut writer = Writer {
        fonts: BTreeMap::new(),
        colors: BTreeMap::new(),
    };
    for element in parsed
        .tree
        .root()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        for key in ["font-family", "color", "background-color"] {
            if let Some(value) = style_value(&element, key) {
                if key == "font-family" {
                    let index = writer.fonts.len();
                    writer.fonts.entry(value).or_insert(index);
                } else {
                    let index = writer.colors.len() + 1;
                    writer.colors.entry(value).or_insert(index);
                }
            }
        }
    }
    let mut out = String::from("{\\rtf1\\ansi\\ansicpg1252\\uc1");
    out.push_str("{\\fonttbl");
    let mut fonts: Vec<_> = writer.fonts.iter().collect();
    fonts.sort_by_key(|(_, i)| **i);
    for (font, index) in fonts {
        let _ = write!(out, "{{\\f{index} ");
        encoded(&font.replace(';', ""), &mut out);
        out.push_str(";}");
    }
    out.push('}');
    out.push_str("{\\colortbl;");
    let mut colors: Vec<_> = writer.colors.iter().collect();
    colors.sort_by_key(|(_, i)| **i);
    for (color, _) in colors {
        let [r, g, b, _] = color
            .parse::<csscolorparser::Color>()
            .context("Invalid export color")?
            .to_rgba8();
        let _ = write!(out, "\\red{r}\\green{g}\\blue{b};");
    }
    out.push('}');
    let p = &document.page;
    let _ = writeln!(
        out,
        "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}",
        p.width_twips, p.height_twips, p.margin_left, p.margin_right, p.margin_top, p.margin_bottom
    );
    writer.children(parsed.root_element(), false, &mut out)?;
    out.push('}');
    ensure!(
        out.len() <= MAX_HTML_BYTES,
        "RTF export exceeds its size limit."
    );
    Ok(out.into_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_scoped_runs_survive_rtf_roundtrip() {
        let doc=RichDocument{html:"<p>plain <b>bold café 😀</b> <a href='https://example.com'>link</a></p><table><tr><td>A</td><td>B</td></tr></table>".into(),page:PageSettings::default()};
        let bytes = write(&doc).unwrap();
        let doc = read(&bytes).unwrap();
        let text = scraper::Html::parse_fragment(&doc.html)
            .root_element()
            .text()
            .collect::<String>();
        assert!(text.contains("bold café 😀"));
        assert!(doc.html.contains("font-weight:bold"));
        assert!(doc.html.contains("https://example.com"));
        assert!(doc.html.contains("<table>"));
        assert_eq!(doc.page.width_twips, 12240);
    }
    #[test]
    fn malformed_groups_binary_and_active_field_links_are_rejected_or_removed() {
        assert!(read(b"{\\rtf1 missing close").is_err());
        assert!(read(b"{\\rtf1 {\\pict\\bin999999999 x}}").is_err());
        let doc = read(
            b"{\\rtf1 {\\field{\\*\\fldinst HYPERLINK \"javascript:bad()\"}{\\fldrslt safe}}}",
        )
        .unwrap();
        assert!(!doc.html.contains("javascript:"));
        assert!(doc.html.contains("safe"));
    }
}
