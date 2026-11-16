//! Offline Office ZIP/XML codecs; all media is embedded and every resource is bounded.
//! shortcut: complex Office layout, revision history, headers, footnotes and floating objects are not preserved; use the original application when exact fidelity is required.
use super::{
    MAX_DEPTH, MAX_HTML_BYTES, MAX_IMAGE_BYTES, MAX_NODES, PageSettings, RichDocument, escape,
    image_data, image_uri, is_safe_href, safe_native_html,
};
use anyhow::{Context, Result, ensure};
use roxmltree::{Document, Node};
use scraper::ElementRef;
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{Cursor, Read as _, Write as _},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_XML_BYTES: usize = 64 * 1024 * 1024;
const WORD: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const OSTYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const XLINK: &str = "http://www.w3.org/1999/xlink";
type Package = BTreeMap<String, Vec<u8>>;
type Style = BTreeMap<String, String>;
fn package(bytes: &[u8]) -> Result<Package> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).context("Opening the document package")?;
    ensure!(
        archive.len() <= 16_384,
        "The document has too many ZIP entries."
    );
    let mut total = 0u64;
    let mut files = Package::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .context("Reading a document package entry")?;
        let name = entry.name().to_owned();
        ensure!(
            !name.starts_with('/')
                && !name.contains(['\\', ':', '\0'])
                && name.split('/').all(|s| s != ".."),
            "Unsafe document package path."
        );
        ensure!(
            entry
                .unix_mode()
                .is_none_or(|mode| mode & 0o170000 != 0o120000),
            "Symbolic links are not allowed in document packages."
        );
        if entry.is_dir() {
            continue;
        }
        let limit = if name.ends_with(".xml") || name.ends_with(".rels") {
            MAX_XML_BYTES as u64
        } else {
            MAX_IMAGE_BYTES
        };
        ensure!(
            entry.size() <= limit,
            "A document package resource is too large."
        );
        total = total
            .checked_add(entry.size())
            .context("Package size overflow")?;
        ensure!(
            total <= MAX_PACKAGE_BYTES,
            "The expanded document package exceeds its resource limit."
        );
        let declared_size = entry.size();
        let mut data = Vec::new();
        entry.take(limit + 1).read_to_end(&mut data)?;
        ensure!(
            data.len() as u64 == declared_size,
            "The package resource length does not match its metadata."
        );
        ensure!(
            data.len() as u64 <= limit,
            "A document package resource exceeds its expanded limit."
        );
        ensure!(
            files.insert(name, data).is_none(),
            "The document package contains duplicate paths."
        );
    }
    Ok(files)
}
fn xml(source: &str) -> Result<Document<'_>> {
    ensure!(
        source.len() <= MAX_XML_BYTES,
        "Document XML exceeds its size limit."
    );
    ensure!(
        !source.contains("<!DOCTYPE") && !source.contains("<!ENTITY"),
        "Document XML cannot contain DTDs or entities."
    );
    let doc = Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES as u32,
            ..Default::default()
        },
    )
    .context("Reading document XML")?;
    ensure!(
        doc.descendants()
            .all(|n| n.ancestors().take(MAX_DEPTH + 2).count() <= MAX_DEPTH),
        "Document XML is nested too deeply."
    );
    Ok(doc)
}
fn text<'a>(files: &'a Package, name: &str) -> Result<&'a str> {
    std::str::from_utf8(
        files
            .get(name)
            .with_context(|| format!("The document is missing {name}"))?,
    )
    .context("Document XML is not UTF-8")
}
fn attr<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}
fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}
fn val<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|n| attr(n, "val"))
}
fn number(node: Node<'_, '_>, name: &str) -> Option<i32> {
    attr(node, name)?.parse().ok()
}
fn safe_package_path(base: &str, target: &str) -> Option<String> {
    let target = percent_encoding::percent_decode_str(target)
        .decode_utf8()
        .ok()?;
    if target.contains(['\\', ':', '\0', '#', '?']) || target.starts_with('/') {
        return None;
    }
    let mut parts: Vec<_> = base.split('/').filter(|s| !s.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}
fn x(value: &str) -> String {
    escape(
        &value
            .chars()
            .filter(|c| matches!(*c, '\t' | '\r' | '\n') || *c >= ' ')
            .collect::<String>(),
    )
}
fn declarations(style: &Style) -> String {
    style
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}
fn span(style: &Style, body: String) -> String {
    if style.is_empty() {
        body
    } else {
        format!(
            "<span style=\"{}\">{body}</span>",
            escape(&declarations(style))
        )
    }
}
fn parse_css(source: &str) -> Style {
    source
        .split(';')
        .filter_map(|s| {
            let (k, v) = s.split_once(':')?;
            Some((k.trim().to_owned(), v.trim().to_owned()))
        })
        .collect()
}
fn word_properties(node: Node<'_, '_>) -> Style {
    let mut style = Style::new();
    for item in node.children().filter(Node::is_element) {
        let value = attr(item, "val").unwrap_or("");
        match item.tag_name().name() {
            "b" if !matches!(value, "0" | "false" | "off") => {
                style.insert("font-weight".into(), "bold".into());
            }
            "i" if !matches!(value, "0" | "false" | "off") => {
                style.insert("font-style".into(), "italic".into());
            }
            "u" if !matches!(value, "none" | "0" | "false") => {
                style.insert("text-decoration".into(), "underline".into());
            }
            "strike" | "dstrike" if !matches!(value, "0" | "false" | "off") => {
                style.insert("text-decoration".into(), "line-through".into());
            }
            "rFonts" => {
                if let Some(font) = attr(item, "ascii").or_else(|| attr(item, "hAnsi")) {
                    style.insert("font-family".into(), font.to_owned());
                }
            }
            "sz" => {
                if let Ok(size) = value.parse::<f32>()
                    && (2.0..=3276.0).contains(&size)
                {
                    style.insert("font-size".into(), format!("{}pt", size / 2.));
                }
            }
            "color" if value.len() == 6 && value.bytes().all(|b| b.is_ascii_hexdigit()) => {
                style.insert("color".into(), format!("#{value}"));
            }
            "shd" => {
                if let Some(fill) = attr(item, "fill")
                    .filter(|s| s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    style.insert("background-color".into(), format!("#{fill}"));
                }
            }
            "highlight" if value != "none" => {
                style.insert("background-color".into(), value.to_owned());
            }
            "vertAlign" if value == "superscript" || value == "subscript" => {
                style.insert(
                    "vertical-align".into(),
                    if value == "superscript" {
                        "super"
                    } else {
                        "sub"
                    }
                    .into(),
                );
            }
            "jc" => {
                style.insert(
                    "text-align".into(),
                    match value {
                        "both" => "justify",
                        v => v,
                    }
                    .to_owned(),
                );
            }
            "ind" => {
                for (name, key) in [("left", "margin-left"), ("firstLine", "text-indent")] {
                    if let Some(n) = number(item, name) {
                        style.insert(key.into(), format!("{}pt", n as f32 / 20.));
                    }
                }
            }
            "bidi" if !matches!(value, "0" | "false" | "off") => {
                style.insert("direction".into(), "rtl".into());
            }
            _ => {}
        }
    }
    style
}
#[derive(Default)]
struct RawStyle {
    base: Option<String>,
    css: Style,
}
fn resolved_styles(raw: BTreeMap<String, RawStyle>) -> BTreeMap<String, Style> {
    let mut result = BTreeMap::new();
    for id in raw.keys() {
        let mut chain = Vec::new();
        let mut next = Some(id.as_str());
        for _ in 0..32 {
            let Some(key) = next else {
                break;
            };
            let Some(item) = raw.get(key) else {
                break;
            };
            chain.push(&item.css);
            next = item.base.as_deref();
        }
        let mut css = Style::new();
        for item in chain.into_iter().rev() {
            css.extend(item.clone());
        }
        result.insert(id.clone(), css);
    }
    result
}
fn word_styles(source: Option<&str>) -> Result<BTreeMap<String, Style>> {
    let Some(source) = source else {
        return Ok(BTreeMap::new());
    };
    let doc = xml(source)?;
    let mut raw = BTreeMap::new();
    for node in doc
        .descendants()
        .filter(|n| n.has_tag_name((WORD, "style")))
    {
        if let Some(id) = attr(node, "styleId") {
            let mut css = Style::new();
            for prop in node
                .children()
                .filter(|n| n.is_element() && matches!(n.tag_name().name(), "pPr" | "rPr"))
            {
                css.extend(word_properties(prop));
            }
            raw.insert(
                id.to_owned(),
                RawStyle {
                    base: val(node, "basedOn").map(str::to_owned),
                    css,
                },
            );
        }
    }
    Ok(resolved_styles(raw))
}
struct Relation {
    target: String,
    external: bool,
}
fn relationships(source: Option<&str>) -> Result<BTreeMap<String, Relation>> {
    let mut map = BTreeMap::new();
    if let Some(source) = source {
        let doc = xml(source)?;
        for node in doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        {
            if let (Some(id), Some(target)) = (attr(node, "Id"), attr(node, "Target")) {
                map.insert(
                    id.into(),
                    Relation {
                        target: target.into(),
                        external: attr(node, "TargetMode") == Some("External"),
                    },
                );
            }
        }
    }
    Ok(map)
}
struct WordReader<'a> {
    files: &'a Package,
    rels: BTreeMap<String, Relation>,
    styles: BTreeMap<String, Style>,
}
impl WordReader<'_> {
    fn render(&self, node: Node<'_, '_>, out: &mut String) -> Result<()> {
        if !node.is_element() {
            return Ok(());
        }
        match node.tag_name().name() {
            "p" => {
                let ppr = child(node, "pPr");
                let id = ppr.and_then(|n| val(n, "pStyle")).unwrap_or("");
                let mut style = self.styles.get(id).cloned().unwrap_or_default();
                if let Some(ppr) = ppr {
                    style.extend(word_properties(ppr));
                }
                let lower = id.to_ascii_lowercase();
                let heading = lower
                    .strip_prefix("heading")
                    .and_then(|s| s.parse::<u8>().ok())
                    .filter(|n| (1..=6).contains(n));
                let tag = heading
                    .map(|n| format!("h{n}"))
                    .unwrap_or_else(|| "p".into());
                let _ = write!(out, "<{tag} style=\"{}\">", escape(&declarations(&style)));
                if ppr.is_some_and(|n| child(n, "numPr").is_some()) {
                    out.push_str("• ");
                }
                for n in node.children() {
                    self.render(n, out)?;
                }
                let _ = write!(out, "</{tag}>");
            }
            "r" => {
                let rpr = child(node, "rPr");
                let id = rpr.and_then(|n| val(n, "rStyle")).unwrap_or("");
                let mut style = self.styles.get(id).cloned().unwrap_or_default();
                if let Some(rpr) = rpr {
                    style.extend(word_properties(rpr));
                }
                let mut inner = String::new();
                for n in node.children() {
                    self.render(n, &mut inner)?;
                }
                out.push_str(&span(&style, inner));
            }
            "t" => out.push_str(&escape(node.text().unwrap_or(""))),
            "tab" => out.push('\t'),
            "br" | "cr" => out.push_str("<br>"),
            "hyperlink" => {
                let href = attr(node, "id")
                    .and_then(|id| self.rels.get(id))
                    .map(|r| r.target.clone())
                    .or_else(|| attr(node, "anchor").map(|a| format!("#{a}")));
                if let Some(href) = href.filter(|s| is_safe_href(s)) {
                    let _ = write!(out, "<a href=\"{}\">", escape(&href));
                    for n in node.children() {
                        self.render(n, out)?;
                    }
                    out.push_str("</a>");
                } else {
                    for n in node.children() {
                        self.render(n, out)?;
                    }
                }
            }
            "drawing" | "pict" => {
                for blip in node.descendants().filter(|n| {
                    n.is_element() && matches!(n.tag_name().name(), "blip" | "imagedata")
                }) {
                    let id = attr(blip, "embed").or_else(|| attr(blip, "id"));
                    if let Some(relation) =
                        id.and_then(|id| self.rels.get(id)).filter(|r| !r.external)
                        && let Some(uri) = safe_package_path("word", &relation.target)
                            .and_then(|name| self.files.get(&name))
                            .and_then(|bytes| image_uri(bytes))
                    {
                        let _ = write!(out, "<img src=\"{uri}\">");
                    }
                }
            }
            "tbl" => {
                out.push_str("<table>");
                for n in node.children() {
                    self.render(n, out)?;
                }
                out.push_str("</table>");
            }
            "tr" => {
                out.push_str("<tr>");
                for n in node.children() {
                    self.render(n, out)?;
                }
                out.push_str("</tr>");
            }
            "tc" => {
                let columns = child(node, "tcPr")
                    .and_then(|n| val(n, "gridSpan"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1)
                    .clamp(1, 1000);
                let _ = write!(out, "<td colspan=\"{columns}\">");
                for n in node.children() {
                    self.render(n, out)?;
                }
                out.push_str("</td>");
            }
            "pPr" | "rPr" | "tblPr" | "tblGrid" | "trPr" | "tcPr" | "sectPr" | "instrText"
            | "del" => {}
            _ => {
                for n in node.children() {
                    self.render(n, out)?;
                }
            }
        }
        ensure!(
            out.len() <= MAX_HTML_BYTES,
            "Word content exceeds its expanded size limit."
        );
        Ok(())
    }
}
pub(super) fn read_docx(bytes: &[u8]) -> Result<RichDocument> {
    let files = package(bytes)?;
    let doc = xml(text(&files, "word/document.xml")?)?;
    let reader = WordReader {
        rels: relationships(
            files
                .get("word/_rels/document.xml.rels")
                .map(|b| std::str::from_utf8(b))
                .transpose()?,
        )?,
        styles: word_styles(
            files
                .get("word/styles.xml")
                .map(|b| std::str::from_utf8(b))
                .transpose()?,
        )?,
        files: &files,
    };
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name((WORD, "body")))
        .context("Word document has no body")?;
    let mut html = String::new();
    reader.render(body, &mut html)?;
    let mut page = PageSettings::default();
    if let Some(section) = doc
        .descendants()
        .rfind(|n| n.has_tag_name((WORD, "sectPr")))
    {
        if let Some(size) = child(section, "pgSz") {
            page.width_twips = number(size, "w").unwrap_or(page.width_twips);
            page.height_twips = number(size, "h").unwrap_or(page.height_twips);
        }
        if let Some(margins) = child(section, "pgMar") {
            page.margin_left = number(margins, "left").unwrap_or(page.margin_left);
            page.margin_right = number(margins, "right").unwrap_or(page.margin_right);
            page.margin_top = number(margins, "top").unwrap_or(page.margin_top);
            page.margin_bottom = number(margins, "bottom").unwrap_or(page.margin_bottom);
        }
    }
    Ok(RichDocument {
        html: safe_native_html(&html)?,
        page,
    })
}
fn odt_properties(node: Node<'_, '_>) -> Style {
    let mut style = Style::new();
    for prop in node.children().filter(Node::is_element) {
        for attribute in prop.attributes() {
            let key = match (attribute.namespace(), attribute.name()) {
                (Some(FO), "font-weight") => "font-weight",
                (Some(FO), "font-style") => "font-style",
                (Some(FO), "font-size") => "font-size",
                (Some(FO), "font-family") => "font-family",
                (Some(FO), "color") => "color",
                (Some(FO), "background-color") => "background-color",
                (Some(FO), "text-align") => "text-align",
                (Some(FO), "margin-left") => "margin-left",
                (Some(FO), "text-indent") => "text-indent",
                (Some(FO), "line-height") => "line-height",
                (Some(OSTYLE), "text-underline-style") if attribute.value() != "none" => {
                    style.insert("text-decoration".into(), "underline".into());
                    continue;
                }
                (Some(OSTYLE), "text-line-through-style") if attribute.value() != "none" => {
                    style.insert("text-decoration".into(), "line-through".into());
                    continue;
                }
                (Some(OSTYLE), "text-position") => {
                    style.insert(
                        "vertical-align".into(),
                        if attribute.value().starts_with("super") {
                            "super"
                        } else {
                            "sub"
                        }
                        .into(),
                    );
                    continue;
                }
                _ => continue,
            };
            style.insert(key.into(), attribute.value().to_owned());
        }
    }
    style
}
fn odt_styles(docs: &[&Document<'_>]) -> BTreeMap<String, Style> {
    let mut raw = BTreeMap::new();
    for doc in docs {
        for node in doc
            .descendants()
            .filter(|n| n.has_tag_name((OSTYLE, "style")))
        {
            if let Some(name) = attr(node, "name") {
                raw.insert(
                    name.to_owned(),
                    RawStyle {
                        base: attr(node, "parent-style-name").map(str::to_owned),
                        css: odt_properties(node),
                    },
                );
            }
        }
    }
    resolved_styles(raw)
}
fn twips(value: &str) -> Option<i32> {
    let split = value.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')?;
    let number = value[..split].parse::<f64>().ok()?;
    let factor = match &value[split..] {
        "in" => 1440.,
        "cm" => 1440. / 2.54,
        "mm" => 1440. / 25.4,
        "pt" => 20.,
        "pc" => 240.,
        "px" => 15.,
        _ => return None,
    };
    let value = number * factor;
    (value.is_finite() && (-100_000.0..=100_000.0).contains(&value)).then_some(value.round() as i32)
}
fn odt_render(
    node: Node<'_, '_>,
    files: &Package,
    styles: &BTreeMap<String, Style>,
    out: &mut String,
) -> Result<()> {
    if node.is_text() {
        out.push_str(&escape(node.text().unwrap_or("")));
        return Ok(());
    }
    if !node.is_element() {
        return Ok(());
    }
    let name = node.tag_name().name();
    let tag = match name {
        "p" => Some("p".into()),
        "h" => Some(format!(
            "h{}",
            attr(node, "outline-level")
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(1)
                .clamp(1, 6)
        )),
        "span" => Some("span".into()),
        "list" => Some("ul".into()),
        "list-item" => Some("li".into()),
        "table" => Some("table".into()),
        "table-row" => Some("tr".into()),
        "table-cell" => Some("td".into()),
        _ => None,
    };
    if let Some(tag) = &tag {
        let _ = write!(out, "<{tag}");
        if let Some(style) = attr(node, "style-name").and_then(|s| styles.get(s)) {
            let _ = write!(out, " style=\"{}\"", escape(&declarations(style)));
        }
        if name == "table-cell" {
            for (key, html) in [
                ("number-columns-spanned", "colspan"),
                ("number-rows-spanned", "rowspan"),
            ] {
                if let Some(span) = attr(node, key) {
                    let _ = write!(out, " {html}=\"{}\"", escape(span));
                }
            }
        }
        out.push('>');
    }
    match name {
        "a" => {
            if let Some(href) = node.attribute((XLINK, "href")).filter(|s| is_safe_href(s)) {
                let _ = write!(out, "<a href=\"{}\">", escape(href));
                for child in node.children() {
                    odt_render(child, files, styles, out)?;
                }
                out.push_str("</a>");
            } else {
                for child in node.children() {
                    odt_render(child, files, styles, out)?;
                }
            }
        }
        "image" => {
            if let Some(bytes) = node
                .attribute((XLINK, "href"))
                .and_then(|name| safe_package_path("", name))
                .and_then(|name| files.get(&name))
                && let Some(uri) = image_uri(bytes)
            {
                let _ = write!(out, "<img src=\"{uri}\">");
            }
        }
        "line-break" => out.push_str("<br>"),
        "tab" => out.push('\t'),
        "s" => {
            let count = attr(node, "c")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1);
            ensure!(
                count <= 10_000,
                "OpenDocument space repetition exceeds its limit."
            );
            out.push_str(&"\u{a0}".repeat(count));
        }
        "covered-table-cell" | "tracked-changes" | "annotation" | "binary-data"
        | "automatic-styles" | "scripts" => {}
        _ => {
            for child in node.children() {
                let repeats = if child.is_element()
                    && matches!(child.tag_name().name(), "table-cell" | "table-row")
                {
                    attr(
                        child,
                        if child.tag_name().name() == "table-cell" {
                            "number-columns-repeated"
                        } else {
                            "number-rows-repeated"
                        },
                    )
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1)
                } else {
                    1
                };
                ensure!(
                    (1..=1000).contains(&repeats),
                    "OpenDocument repetition exceeds its limit."
                );
                for _ in 0..repeats {
                    odt_render(child, files, styles, out)?;
                }
            }
        }
    }
    if let Some(tag) = tag {
        let _ = write!(out, "</{tag}>");
    }
    ensure!(
        out.len() <= MAX_HTML_BYTES,
        "OpenDocument content exceeds its expanded size limit."
    );
    Ok(())
}
pub(super) fn read_odt(bytes: &[u8]) -> Result<RichDocument> {
    let files = package(bytes)?;
    let doc = xml(text(&files, "content.xml")?)?;
    let styles_doc = files
        .get("styles.xml")
        .map(|bytes| std::str::from_utf8(bytes))
        .transpose()?
        .map(xml)
        .transpose()?;
    let mut docs = Vec::new();
    if let Some(styles) = &styles_doc {
        docs.push(styles);
    }
    docs.push(&doc);
    let styles = odt_styles(&docs);
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name(("urn:oasis:names:tc:opendocument:xmlns:office:1.0", "text")))
        .context("OpenDocument has no text body")?;
    let mut html = String::new();
    odt_render(body, &files, &styles, &mut html)?;
    let mut page = PageSettings::default();
    if let Some(style) = styles_doc.as_ref().and_then(|d| {
        d.descendants()
            .find(|n| n.has_tag_name((OSTYLE, "page-layout-properties")))
    }) {
        for attribute in style.attributes() {
            let Some(value) = twips(attribute.value()) else {
                continue;
            };
            match attribute.name() {
                "page-width" => page.width_twips = value,
                "page-height" => page.height_twips = value,
                "margin-left" => page.margin_left = value,
                "margin-right" => page.margin_right = value,
                "margin-top" => page.margin_top = value,
                "margin-bottom" => page.margin_bottom = value,
                "margin" => {
                    page.margin_left = value;
                    page.margin_right = value;
                    page.margin_top = value;
                    page.margin_bottom = value;
                }
                _ => {}
            }
        }
    }
    Ok(RichDocument {
        html: safe_native_html(&html)?,
        page,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    Odt,
}
struct Media {
    name: String,
    bytes: Vec<u8>,
    mime: &'static str,
}
struct Exporter {
    kind: Kind,
    rels: Vec<String>,
    media: Vec<Media>,
    styles: Vec<String>,
    image_id: usize,
}
fn html_style(element: ElementRef<'_>, parent: &Style) -> Style {
    let mut style = parent.clone();
    let item = match element.value().name() {
        "b" | "strong" => Some(("font-weight", "bold")),
        "i" | "em" => Some(("font-style", "italic")),
        "u" => Some(("text-decoration", "underline")),
        "s" | "strike" | "del" => Some(("text-decoration", "line-through")),
        "sub" => Some(("vertical-align", "sub")),
        "sup" => Some(("vertical-align", "super")),
        "code" => Some(("font-family", "monospace")),
        _ => None,
    };
    if let Some((key, value)) = item {
        style.insert(key.into(), value.into());
    }
    if let Some(css) = element.value().attr("style") {
        style.extend(parse_css(css));
    }
    style
}
fn points(value: &str) -> Option<f64> {
    if let Some(twips) = twips(value) {
        Some(f64::from(twips) / 20.)
    } else {
        value.parse().ok()
    }
}
fn color(value: &str) -> Option<String> {
    let [r, g, b, _] = value.parse::<csscolorparser::Color>().ok()?.to_rgba8();
    Some(format!("{r:02X}{g:02X}{b:02X}"))
}
fn word_run_properties(style: &Style) -> String {
    let mut out = String::new();
    for (key, value) in style {
        match key.as_str() {
            "font-weight" if value == "bold" || value.parse::<i32>().is_ok_and(|n| n >= 600) => {
                out.push_str("<w:b/>")
            }
            "font-style" if value == "italic" => out.push_str("<w:i/>"),
            "text-decoration" if value.contains("underline") => {
                out.push_str("<w:u w:val=\"single\"/>")
            }
            "text-decoration" if value.contains("line-through") => out.push_str("<w:strike/>"),
            "font-family" => {
                let _ = write!(
                    out,
                    "<w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>",
                    x(value),
                    x(value)
                );
            }
            "font-size" => {
                if let Some(size) = points(value).filter(|n| n.is_finite()) {
                    let _ = write!(
                        out,
                        "<w:sz w:val=\"{}\"/>",
                        (size * 2.).round().clamp(2., 3276.) as i32
                    );
                }
            }
            "color" => {
                if let Some(value) = color(value) {
                    let _ = write!(out, "<w:color w:val=\"{value}\"/>");
                }
            }
            "background-color" => {
                if let Some(value) = color(value) {
                    let _ = write!(out, "<w:shd w:fill=\"{value}\"/>");
                }
            }
            "vertical-align" if matches!(value.as_str(), "sub" | "super") => {
                let _ = write!(
                    out,
                    "<w:vertAlign w:val=\"{}script\"/>",
                    if value == "sub" { "sub" } else { "super" }
                );
            }
            _ => {}
        }
    }
    if out.is_empty() {
        out
    } else {
        format!("<w:rPr>{out}</w:rPr>")
    }
}
fn word_paragraph_properties(style: &Style, list: Option<bool>, heading: Option<u8>) -> String {
    let mut out = String::new();
    if let Some(heading) = heading {
        let _ = write!(out, "<w:pStyle w:val=\"Heading{heading}\"/>");
    }
    if let Some(ordered) = list {
        let id = if ordered { 2 } else { 1 };
        let _ = write!(
            out,
            "<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"{id}\"/></w:numPr>"
        );
    }
    for (key, value) in style {
        match key.as_str() {
            "text-align" => {
                let value = if value == "justify" { "both" } else { value };
                let _ = write!(out, "<w:jc w:val=\"{}\"/>", x(value));
            }
            "margin-left" | "text-indent" => {
                if let Some(number) = points(value).filter(|n| n.is_finite()) {
                    let _ = write!(
                        out,
                        "<w:ind w:{}=\"{}\"/>",
                        if key == "margin-left" {
                            "left"
                        } else {
                            "firstLine"
                        },
                        (number * 20.).round().clamp(-28800., 28800.) as i32
                    );
                }
            }
            "direction" if value == "rtl" => out.push_str("<w:bidi/>"),
            _ => {}
        }
    }
    if out.is_empty() {
        out
    } else {
        format!("<w:pPr>{out}</w:pPr>")
    }
}
impl Exporter {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            rels: Vec::new(),
            media: Vec::new(),
            styles: Vec::new(),
            image_id: 0,
        }
    }
    fn relationship(&mut self, kind: &str, target: &str, external: bool) -> String {
        let id = format!("rId{}", self.rels.len() + 1);
        self.rels.push(format!(
            "<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{}\"{}/>",
            x(target),
            if external {
                " TargetMode=\"External\""
            } else {
                ""
            }
        ));
        id
    }
    fn odt_style(&mut self, style: &Style, paragraph: bool) -> String {
        let id = format!("S{}", self.styles.len() + 1);
        let mut text = String::new();
        let mut para = String::new();
        for (key, value) in style {
            match key.as_str() {
                "font-weight" | "font-style" | "font-family" | "font-size" | "color"
                | "background-color" => {
                    let _ = write!(text, " fo:{key}=\"{}\"", x(value));
                }
                "text-decoration" if value.contains("underline") => {
                    text.push_str(" style:text-underline-style=\"solid\"")
                }
                "text-decoration" if value.contains("line-through") => {
                    text.push_str(" style:text-line-through-style=\"solid\"")
                }
                "vertical-align" if matches!(value.as_str(), "sub" | "super") => {
                    let _ = write!(text, " style:text-position=\"{} 58%\"", x(value));
                }
                "text-align" | "margin-left" | "text-indent" | "line-height" => {
                    let _ = write!(para, " fo:{key}=\"{}\"", x(value));
                }
                _ => {}
            }
        }
        self.styles.push(format!("<style:style style:name=\"{id}\" style:family=\"{}\"><style:text-properties{text}/>{}</style:style>",if paragraph{"paragraph"}else{"text"},if paragraph{format!("<style:paragraph-properties{para}/>")}else{String::new()}));
        id
    }
    fn text(&mut self, value: &str, style: &Style, out: &mut String) {
        if self.kind == Kind::Word {
            let _ = write!(
                out,
                "<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                word_run_properties(style),
                x(value)
            );
        } else {
            let id = self.odt_style(style, false);
            let _ = write!(
                out,
                "<text:span text:style-name=\"{id}\">{}</text:span>",
                x(value)
                    .replace('\t', "<text:tab/>")
                    .replace('\n', "<text:line-break/>")
            );
        }
    }
    fn image(&mut self, element: ElementRef<'_>, out: &mut String) -> Result<()> {
        let Some(bytes) = element.value().attr("src").and_then(image_data) else {
            return Ok(());
        };
        let image = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        let (width, height) = image.into_dimensions()?;
        let (ext, mime, bytes) = if bytes.starts_with(b"\x89PNG") {
            ("png", "image/png", bytes)
        } else if bytes.starts_with(b"\xff\xd8") {
            ("jpg", "image/jpeg", bytes)
        } else {
            let image = image::load_from_memory(&bytes)?;
            let mut cursor = Cursor::new(Vec::new());
            image.write_to(&mut cursor, image::ImageFormat::Png)?;
            ("png", "image/png", cursor.into_inner())
        };
        ensure!(
            self.media.iter().map(|m| m.bytes.len() as u64).sum::<u64>() + bytes.len() as u64
                <= MAX_HTML_BYTES as u64,
            "Export images exceed their resource limit."
        );
        self.image_id += 1;
        let id = self.image_id;
        let width = element
            .value()
            .attr("width")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(width)
            .clamp(1, 32768);
        let height = element
            .value()
            .attr("height")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(height)
            .clamp(1, 32768);
        if self.kind == Kind::Word {
            let name = format!("word/media/image{id}.{ext}");
            let rel = self.relationship("image", &format!("media/image{id}.{ext}"), false);
            self.media.push(Media { name, bytes, mime });
            let (cx, cy) = (u64::from(width) * 9525, u64::from(height) * 9525);
            let _ = write!(
                out,
                "<w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"Picture {id}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
            );
        } else {
            let name = format!("Pictures/image{id}.{ext}");
            let _ = write!(
                out,
                "<draw:frame draw:name=\"Picture {id}\" text:anchor-type=\"as-char\" svg:width=\"{}in\" svg:height=\"{}in\"><draw:image xlink:href=\"{name}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>",
                f64::from(width) / 96.,
                f64::from(height) / 96.
            );
            self.media.push(Media { name, bytes, mime });
        }
        Ok(())
    }
    fn inline(&mut self, element: ElementRef<'_>, style: &Style, out: &mut String) -> Result<()> {
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                self.text(text, style, out);
                continue;
            }
            let Some(element) = ElementRef::wrap(child) else {
                continue;
            };
            let tag = element.value().name();
            let style = html_style(element, style);
            match tag {
                "br" => out.push_str(if self.kind == Kind::Word {
                    "<w:r><w:br/></w:r>"
                } else {
                    "<text:line-break/>"
                }),
                "img" => self.image(element, out)?,
                "a" => {
                    if let Some(href) = element.value().attr("href").filter(|s| is_safe_href(s)) {
                        if self.kind == Kind::Word {
                            let rel = self.relationship("hyperlink", href, true);
                            let _ = write!(out, "<w:hyperlink r:id=\"{rel}\">");
                            self.inline(element, &style, out)?;
                            out.push_str("</w:hyperlink>");
                        } else {
                            let _ = write!(
                                out,
                                "<text:a xlink:href=\"{}\" xlink:type=\"simple\">",
                                x(href)
                            );
                            self.inline(element, &style, out)?;
                            out.push_str("</text:a>");
                        }
                    } else {
                        self.inline(element, &style, out)?;
                    }
                }
                "ul" | "ol" | "table" => {}
                _ => self.inline(element, &style, out)?,
            }
        }
        Ok(())
    }
    fn paragraph(
        &mut self,
        element: ElementRef<'_>,
        list: Option<bool>,
        out: &mut String,
    ) -> Result<()> {
        let mut style = html_style(element, &Style::new());
        let heading = element
            .value()
            .name()
            .strip_prefix('h')
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=6).contains(n));
        if let Some(heading) = heading {
            style
                .entry("font-weight".into())
                .or_insert_with(|| "bold".into());
            style
                .entry("font-size".into())
                .or_insert_with(|| format!("{}pt", 28u8.saturating_sub(heading * 3)));
        }
        if self.kind == Kind::Word {
            let _ = write!(
                out,
                "<w:p>{}",
                word_paragraph_properties(&style, list, heading)
            );
            self.inline(element, &style, out)?;
            out.push_str("</w:p>");
        } else {
            let id = self.odt_style(&style, true);
            let tag = if heading.is_some() {
                "text:h"
            } else {
                "text:p"
            };
            let _ = write!(
                out,
                "<{tag} text:style-name=\"{id}\"{}>",
                heading
                    .map(|h| format!(" text:outline-level=\"{h}\""))
                    .unwrap_or_default()
            );
            self.inline(element, &style, out)?;
            let _ = write!(out, "</{tag}>");
        }
        Ok(())
    }
    fn list(&mut self, element: ElementRef<'_>, out: &mut String) -> Result<()> {
        let ordered = element.value().name() == "ol";
        if self.kind == Kind::Odt {
            let _ = write!(
                out,
                "<text:list text:style-name=\"{}\">",
                if ordered { "Numbering" } else { "Bullet" }
            );
        }
        for item in element
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "li")
        {
            if self.kind == Kind::Odt {
                out.push_str("<text:list-item>");
            }
            let paragraphs: Vec<_> = item
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|e| e.value().name() == "p")
                .collect();
            if paragraphs.is_empty() {
                self.paragraph(item, Some(ordered), out)?;
            } else {
                for paragraph in paragraphs {
                    self.paragraph(paragraph, Some(ordered), out)?;
                }
            }
            for nested in item
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|e| matches!(e.value().name(), "ul" | "ol"))
            {
                self.list(nested, out)?;
            }
            if self.kind == Kind::Odt {
                out.push_str("</text:list-item>");
            }
        }
        if self.kind == Kind::Odt {
            out.push_str("</text:list>");
        }
        Ok(())
    }
    fn table(&mut self, element: ElementRef<'_>, out: &mut String) -> Result<()> {
        out.push_str(if self.kind == Kind::Word {
            "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr>"
        } else {
            "<table:table>"
        });
        for row in element
            .descendants()
            .filter_map(ElementRef::wrap)
            .filter(|e| {
                e.value().name() == "tr"
                    && e.ancestors()
                        .filter_map(ElementRef::wrap)
                        .find(|e| e.value().name() == "table")
                        .is_some_and(|table| table.id() == element.id())
            })
        {
            out.push_str(if self.kind == Kind::Word {
                "<w:tr>"
            } else {
                "<table:table-row>"
            });
            for cell in row
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|e| matches!(e.value().name(), "td" | "th"))
            {
                let colspan = cell
                    .value()
                    .attr("colspan")
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1)
                    .clamp(1, 1000);
                if self.kind == Kind::Word {
                    if colspan > 1 {
                        let _ = write!(
                            out,
                            "<w:tc><w:tcPr><w:gridSpan w:val=\"{colspan}\"/></w:tcPr>"
                        );
                    } else {
                        out.push_str("<w:tc>");
                    }
                } else if colspan > 1 {
                    let _ = write!(
                        out,
                        "<table:table-cell table:number-columns-spanned=\"{colspan}\" office:value-type=\"string\">"
                    );
                } else {
                    out.push_str("<table:table-cell office:value-type=\"string\">");
                }
                let has_blocks = cell
                    .children()
                    .filter_map(ElementRef::wrap)
                    .any(|e| matches!(e.value().name(), "p" | "div" | "table"));
                if has_blocks {
                    self.blocks(cell, out)?;
                } else {
                    self.paragraph(cell, None, out)?;
                }
                out.push_str(if self.kind == Kind::Word {
                    "</w:tc>"
                } else {
                    "</table:table-cell>"
                });
                if self.kind == Kind::Odt {
                    for _ in 1..colspan {
                        out.push_str("<table:covered-table-cell/>");
                    }
                }
            }
            out.push_str(if self.kind == Kind::Word {
                "</w:tr>"
            } else {
                "</table:table-row>"
            });
        }
        out.push_str(if self.kind == Kind::Word {
            "</w:tbl>"
        } else {
            "</table:table>"
        });
        Ok(())
    }
    fn blocks(&mut self, element: ElementRef<'_>, out: &mut String) -> Result<()> {
        for child in element.children() {
            if let Some(text) = child.value().as_text() {
                if !text.trim().is_empty() {
                    out.push_str(if self.kind == Kind::Word {
                        "<w:p>"
                    } else {
                        "<text:p>"
                    });
                    self.text(text, &Style::new(), out);
                    out.push_str(if self.kind == Kind::Word {
                        "</w:p>"
                    } else {
                        "</text:p>"
                    });
                }
                continue;
            }
            let Some(element) = ElementRef::wrap(child) else {
                continue;
            };
            match element.value().name() {
                "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "pre" => {
                    self.paragraph(element, None, out)?
                }
                "table" => self.table(element, out)?,
                "ul" | "ol" => self.list(element, out)?,
                "img" => {
                    out.push_str(if self.kind == Kind::Word {
                        "<w:p>"
                    } else {
                        "<text:p>"
                    });
                    self.image(element, out)?;
                    out.push_str(if self.kind == Kind::Word {
                        "</w:p>"
                    } else {
                        "</text:p>"
                    });
                }
                "div" | "blockquote" => self.blocks(element, out)?,
                _ => self.paragraph(element, None, out)?,
            }
            ensure!(
                out.len() <= MAX_XML_BYTES,
                "Export XML exceeds its size limit."
            );
        }
        Ok(())
    }
}
fn zip_files(files: Vec<(String, Vec<u8>)>, stored_first: bool) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (index, (name, bytes)) in files.into_iter().enumerate() {
        writer.start_file(
            name,
            SimpleFileOptions::default().compression_method(if stored_first && index == 0 {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            }),
        )?;
        writer.write_all(&bytes)?;
    }
    Ok(writer.finish()?.into_inner())
}
const ODT_NAMESPACES: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"";
pub(super) fn write_docx(document: &RichDocument) -> Result<Vec<u8>> {
    let html = safe_native_html(&document.html)?;
    let parsed = scraper::Html::parse_fragment(&html);
    let mut exporter = Exporter::new(Kind::Word);
    let mut body = String::new();
    exporter.blocks(parsed.root_element(), &mut body)?;
    exporter.relationship("styles", "styles.xml", false);
    exporter.relationship("numbering", "numbering.xml", false);
    let p = &document.page;
    let _ = write!(
        body,
        "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:left=\"{}\" w:right=\"{}\" w:top=\"{}\" w:bottom=\"{}\"/></w:sectPr>",
        p.width_twips, p.height_twips, p.margin_left, p.margin_right, p.margin_top, p.margin_bottom
    );
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{WORD}\" xmlns:r=\"{REL}\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{body}</w:body></w:document>"
    );
    let types = String::from(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/></Types>",
    );
    let mut styles = format!(
        "<w:styles xmlns:w=\"{WORD}\"><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style>"
    );
    for heading in 1..=6 {
        let _ = write!(
            styles,
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{heading}\"><w:name w:val=\"heading {heading}\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>",
            heading - 1
        );
    }
    styles.push_str("</w:styles>");
    let numbering = format!(
        "<w:numbering xmlns:w=\"{WORD}\"><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"•\"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl></w:abstractNum><w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>"
    );
    let mut files=vec![("[Content_Types].xml".into(),types.into_bytes()),("_rels/.rels".into(),format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL}/officeDocument\" Target=\"word/document.xml\"/></Relationships>").into_bytes()),("word/document.xml".into(),document.into_bytes()),("word/styles.xml".into(),styles.into_bytes()),("word/numbering.xml".into(),numbering.into_bytes()),("word/_rels/document.xml.rels".into(),format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",exporter.rels.join("")).into_bytes())];
    for media in exporter.media {
        files.push((media.name, media.bytes));
    }
    zip_files(files, false)
}
pub(super) fn write_odt(document: &RichDocument) -> Result<Vec<u8>> {
    let html = safe_native_html(&document.html)?;
    let parsed = scraper::Html::parse_fragment(&html);
    let mut exporter = Exporter::new(Kind::Odt);
    let mut body = String::new();
    exporter.blocks(parsed.root_element(), &mut body)?;
    let lists = "<text:list-style style:name=\"Bullet\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"•\"/></text:list-style><text:list-style style:name=\"Numbering\"><text:list-level-style-number text:level=\"1\" style:num-format=\"1\"/></text:list-style>";
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {ODT_NAMESPACES} office:version=\"1.3\"><office:automatic-styles>{}{lists}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>",
        exporter.styles.join("")
    );
    let p = &document.page;
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {ODT_NAMESPACES} office:version=\"1.3\"><office:automatic-styles><style:page-layout style:name=\"Page\"><style:page-layout-properties fo:page-width=\"{}pt\" fo:page-height=\"{}pt\" fo:margin-left=\"{}pt\" fo:margin-right=\"{}pt\" fo:margin-top=\"{}pt\" fo:margin-bottom=\"{}pt\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"Page\"/></office:master-styles></office:document-styles>",
        p.width_twips as f64 / 20.,
        p.height_twips as f64 / 20.,
        p.margin_left as f64 / 20.,
        p.margin_right as f64 / 20.,
        p.margin_top as f64 / 20.,
        p.margin_bottom as f64 / 20.
    );
    let mut manifest = String::from(
        "<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>",
    );
    let mut files = vec![
        (
            "mimetype".into(),
            b"application/vnd.oasis.opendocument.text".to_vec(),
        ),
        ("content.xml".into(), content.into_bytes()),
        ("styles.xml".into(), styles.into_bytes()),
    ];
    for media in exporter.media {
        let _ = write!(
            manifest,
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
            x(&media.name),
            media.mime
        );
        files.push((media.name, media.bytes));
    }
    manifest.push_str("</manifest:manifest>");
    files.push(("META-INF/manifest.xml".into(), manifest.into_bytes()));
    zip_files(files, true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn office_roundtrips_unicode_styles_tables_links_images_and_page_settings() {
        let mut image = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 255, 0, 255]))
            .write_to(&mut image, image::ImageFormat::Png)
            .unwrap();
        let uri = image_uri(&image.into_inner()).unwrap();
        let page = PageSettings {
            margin_left: 1440,
            ..Default::default()
        };
        let doc = RichDocument {
            html: format!(
                "<h2>Heading</h2><p><b>bold café 😀</b> <a href='https://example.com'>link</a><img src='{uri}'></p><table><tr><td>A</td><td>B</td></tr></table>"
            ),
            page,
        };
        for kind in [Kind::Word, Kind::Odt] {
            let bytes = if kind == Kind::Word {
                write_docx(&doc)
            } else {
                write_odt(&doc)
            }
            .unwrap();
            let loaded = if kind == Kind::Word {
                read_docx(&bytes)
            } else {
                read_odt(&bytes)
            }
            .unwrap();
            assert!(loaded.html.contains("bold café 😀"));
            assert!(loaded.html.contains("font-weight:bold"));
            assert!(loaded.html.contains("https://example.com"));
            assert!(loaded.html.contains("<table"));
            assert!(loaded.html.contains("data:image/png;base64,"));
            assert_eq!(loaded.page.margin_left, 1440);
        }
    }
    #[test]
    fn package_and_xml_reject_traversal_entities_and_external_relationships() {
        let bytes = zip_files(vec![("../private.xml".into(), b"<x/>".to_vec())], false).unwrap();
        assert!(package(&bytes).is_err());
        assert!(xml("<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///secret'>]><x>&e;</x>").is_err());
        assert!(xml(&format!("{}{}", "<x>".repeat(140), "</x>".repeat(140))).is_err());
        assert!(safe_package_path("word", "../../secret").is_none());
        assert!(safe_package_path("word", "https://example.com/image.png").is_none());
        assert_eq!(
            safe_package_path("word", "media/image.png").as_deref(),
            Some("word/media/image.png")
        );
    }
}
