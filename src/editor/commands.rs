//! App-specific operations registered as WordCraft transactions, so failures roll back with undo.
use super::{PageSettings, core_positions::TextMap};
use serde_json::{Value, json};
use std::sync::Arc;
use wordcraft_doc::{CharProps, Document, StoryRef, edit::Fragment};
use wordcraft_engine::{CmdError, CmdResult, CommandSpec, Registry, Selection, Session};

pub(super) fn install(session: &mut Session) {
    let mut specs = session.registry.all().to_vec();
    specs.extend([
        CommandSpec::new("exeditor.replace", "Replace Text", "Editing", replace_text),
        CommandSpec::new(
            "exeditor.plainFormat",
            "Format Plain Document",
            "Format",
            plain_format,
        ),
        CommandSpec::new("exeditor.editLines", "Edit Lines", "Editing", edit_lines),
        CommandSpec::new(
            "exeditor.pasteDocument",
            "Paste Formatted Content",
            "Editing",
            paste_document,
        ),
        CommandSpec::new(
            "exeditor.pageSettings",
            "Page Setup",
            "Format",
            page_settings,
        ),
    ]);
    session.registry = Arc::new(Registry::new(specs));
}
fn invalid(error: impl std::fmt::Display) -> CmdError {
    CmdError::Params(error.to_string())
}
fn number(value: &Value, key: &str) -> Result<usize, CmdError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| invalid(format!("invalid {key}")))
}
fn range(value: &Value) -> Result<std::ops::Range<usize>, CmdError> {
    let range = number(value, "start")?..number(value, "end")?;
    if range.start > range.end {
        return Err(invalid("the range is reversed"));
    }
    Ok(range)
}
fn text_fragment(text: &str, props: &CharProps) -> Fragment {
    let mut fragment = Fragment::from_text(text);
    for block in &mut fragment.blocks {
        if let wordcraft_doc::Block::Para(p) = block {
            *p = wordcraft_doc::Paragraph::with_text(&p.text, props.clone());
        }
    }
    fragment
}
fn replace_text(s: &mut Session, v: &Value) -> CmdResult {
    let text = v["text"]
        .as_str()
        .ok_or_else(|| invalid("replacement text is missing"))?;
    super::validate_text(text).map_err(invalid)?;
    let props = s.typing_props();
    let paragraph = s
        .doc
        .para_at(&s.sel.ordered().0)
        .map(|p| p.props.clone())
        .unwrap_or_default();
    let mut frag = text_fragment(text, &props);
    for block in &mut frag.blocks {
        if let wordcraft_doc::Block::Para(p) = block {
            p.props = paragraph.clone();
        }
    }
    let (a, b) = s.sel.ordered();
    let at = s.doc.delete_range(&a, &b)?;
    let end = s.doc.insert_fragment(&at, &frag)?;
    s.sel = Selection::caret(end);
    Ok(json!({}))
}
fn plain_format(s: &mut Session, v: &Value) -> CmdResult {
    let id = v["command"]
        .as_str()
        .ok_or_else(|| invalid("a formatting command is required"))?;
    if !id.starts_with("format.") || id == "format.changeCase" {
        return Err(invalid("invalid plain-document formatting command"));
    }
    let spec = s
        .registry
        .get(id)
        .cloned()
        .ok_or_else(|| invalid("unknown character formatting command"))?;
    let selection = s.sel.clone();
    s.sel = Selection {
        anchor: s.doc.start_of(StoryRef::Body),
        focus: s.doc.end_of(StoryRef::Body),
    };
    (spec.run)(s, &v["params"])?;
    let props = s.caret_props();
    Arc::make_mut(&mut s.doc.styles).default_chr.overlay(&props);
    s.sel = selection;
    s.pending = None;
    Ok(json!({}))
}
fn edit_lines(s: &mut Session, v: &Value) -> CmdResult {
    let map = TextMap::new(&s.doc);
    let replaced = range(&v["range"])?;
    let wanted = range(&v["selection"])?;
    let a = map.position(replaced.start).map_err(invalid)?;
    let b = map.position(replaced.end).map_err(invalid)?;
    let pieces = v["pieces"]
        .as_array()
        .ok_or_else(|| invalid("line pieces are missing"))?;
    let props = s.typing_props();
    let mut fragments = Vec::with_capacity(pieces.len());
    let mut expected = String::new();
    for piece in pieces {
        if let Some(text) = piece.get("text").and_then(Value::as_str) {
            expected.push_str(text);
            fragments.push(text_fragment(text, &props));
        } else {
            let source = range(piece)?;
            expected.push_str(map.slice(source.clone()).map_err(invalid)?);
            let first = map.position(source.start).map_err(invalid)?;
            let last = map.position(source.end).map_err(invalid)?;
            fragments.push(s.doc.copy_range(&first, &last));
        }
    }
    if Some(expected.as_str()) != v["replacement"].as_str() {
        return Err(invalid("line fragments do not match their replacement"));
    }
    let mut expected_document = map.text.clone();
    let byte_start = super::core_positions::utf16_to_byte(&map.text, replaced.start)
        .ok_or_else(|| invalid("invalid edit start"))?;
    let byte_end = super::core_positions::utf16_to_byte(&map.text, replaced.end)
        .ok_or_else(|| invalid("invalid edit end"))?;
    expected_document.replace_range(byte_start..byte_end, &expected);
    if expected_document.len() > super::MAX_DOCUMENT_BYTES {
        return Err(invalid("the line edit is too large"));
    }
    let mut at = s.doc.delete_range(&a, &b)?;
    for fragment in fragments {
        at = s.doc.insert_fragment(&at, &fragment)?;
    }
    let result = TextMap::new(&s.doc);
    if result.text != expected_document {
        return Err(CmdError::Failed(
            "the line edit could not preserve its exact rich fragments".into(),
        ));
    }
    s.sel = Selection {
        anchor: result.position(wanted.start).map_err(invalid)?,
        focus: result.position(wanted.end).map_err(invalid)?,
    };
    s.pending = None;
    Ok(json!({}))
}
fn paste_document(s: &mut Session, v: &Value) -> CmdResult {
    let doc: Document = serde_json::from_value(v["document"].clone()).map_err(invalid)?;
    let (a, b) = s.sel.ordered();
    let frag = s.doc.adopt(doc);
    let at = s.doc.delete_range(&a, &b)?;
    let end = s.doc.insert_fragment(&at, &frag)?;
    s.sel = Selection::caret(end);
    s.pending = None;
    Ok(json!({}))
}
fn page_settings(s: &mut Session, v: &Value) -> CmdResult {
    let field = |name| {
        v[name]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| invalid(format!("invalid {name}")))
    };
    let settings = PageSettings {
        width_twips: field("width_twips")?,
        height_twips: field("height_twips")?,
        margin_left: field("margin_left")?,
        margin_right: field("margin_right")?,
        margin_top: field("margin_top")?,
        margin_bottom: field("margin_bottom")?,
    };
    super::printing::apply_page_settings(&mut s.doc, &settings).map_err(invalid)?;
    Ok(json!({}))
}

/// Validate the whole stream before a permissive format reader can accept a truncated document.
pub(super) fn validate_rtf(bytes: &[u8]) -> anyhow::Result<()> {
    use anyhow::ensure;
    ensure!(
        bytes.len() <= super::MAX_DOCUMENT_BYTES,
        "the rich-text document is too large"
    );
    ensure!(
        bytes.starts_with(b"{\\rtf"),
        "the document is not a valid RTF stream"
    );
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut ended = false;
    while i < bytes.len() {
        let b = bytes[i];
        if ended {
            ensure!(
                b.is_ascii_whitespace() || b == 0,
                "unexpected data after the RTF document"
            );
            i += 1;
            continue;
        }
        i += 1;
        match b {
            b'{' => {
                depth += 1;
                ensure!(depth <= 256, "the RTF nesting is too deep");
            }
            b'}' => {
                ensure!(depth > 0, "unbalanced RTF groups");
                depth -= 1;
                ended = depth == 0;
            }
            b'\\' => {
                let next = *bytes
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("truncated RTF escape"))?;
                if next == b'\'' {
                    ensure!(
                        bytes
                            .get(i + 1..i + 3)
                            .is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)),
                        "invalid RTF character escape"
                    );
                    i += 3;
                } else if next.is_ascii_alphabetic() {
                    let start = i;
                    while bytes.get(i).is_some_and(u8::is_ascii_alphabetic) {
                        i += 1;
                    }
                    let word = &bytes[start..i];
                    let negative = bytes.get(i) == Some(&b'-');
                    if negative {
                        i += 1;
                    }
                    let digits = i;
                    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                        i += 1;
                    }
                    let number = &bytes[digits..i];
                    if bytes.get(i) == Some(&b' ') {
                        i += 1;
                    }
                    if word == b"bin" {
                        ensure!(!negative && !number.is_empty(), "invalid RTF binary length");
                        let count: usize = std::str::from_utf8(number)?.parse()?;
                        i = i
                            .checked_add(count)
                            .filter(|end| *end <= bytes.len())
                            .ok_or_else(|| anyhow::anyhow!("truncated RTF binary data"))?;
                    }
                } else {
                    i += 1;
                }
            }
            _ => {}
        }
    }
    ensure!(ended && depth == 0, "the RTF document is incomplete");
    Ok(())
}
