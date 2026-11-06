//! Validated Windows CF_HTML fragments with only their formatting ancestors retained.

use anyhow::{Context, Result, ensure};
use scraper::{ElementRef, Html};
use std::collections::BTreeMap;

pub const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Decodes CF_HTML byte offsets without importing any surrounding document text.
///
/// # Errors
/// Rejects oversized data, malformed headers, invalid UTF-8, and inconsistent ranges.
pub fn decode(bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "Clipboard HTML exceeds the 16 MiB safety limit."
    );
    let end = bytes
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |end| end + 1);
    let text = std::str::from_utf8(&bytes[..end]).context("Clipboard HTML is not valid UTF-8")?;
    ensure!(
        !text.contains('\0'),
        "Clipboard HTML contains an embedded NUL."
    );
    let mut headers = BTreeMap::new();
    let mut cursor = 0;
    let mut content_start = text.len();
    while cursor < content_start {
        let rest = &text.as_bytes()[cursor..content_start];
        let complete = [
            "Version",
            "StartHTML",
            "EndHTML",
            "StartFragment",
            "EndFragment",
        ]
        .iter()
        .all(|key| headers.contains_key(key));
        let length = rest.iter().position(|byte| matches!(byte, b'\r' | b'\n'));
        let Some(length) = length else {
            ensure!(complete, "Clipboard HTML has an unterminated header.");
            break;
        };
        let line = &text[cursor..cursor + length];
        let header_name = line.split_once(':').map(|(key, _)| key);
        if complete
            && !line.is_empty()
            && !header_name.is_some_and(|key| {
                !key.is_empty()
                    && key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            ensure!(
                ![
                    "Version",
                    "StartHTML",
                    "EndHTML",
                    "StartFragment",
                    "EndFragment",
                    "StartSelection",
                    "EndSelection"
                ]
                .iter()
                .any(|key| line.starts_with(key)),
                "Clipboard HTML has a malformed header."
            );
            break;
        }
        cursor += length + 1;
        if text.as_bytes()[cursor - 1] == b'\r' && text.as_bytes().get(cursor) == Some(&b'\n') {
            cursor += 1;
        }
        ensure!(line.is_ascii(), "Clipboard HTML headers must be ASCII.");
        if line.is_empty() {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .context("Clipboard HTML has a malformed header")?;
        ensure!(
            !key.is_empty()
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "Clipboard HTML has an invalid header name."
        );
        let value = value.trim();
        ensure!(
            headers.insert(key, value).is_none(),
            "Clipboard HTML has a duplicate {key} header."
        );
        if matches!(key, "StartHTML" | "StartFragment") && value != "-1" {
            content_start = content_start.min(offset(value)?);
            ensure!(
                cursor <= content_start && content_start <= text.len(),
                "Clipboard HTML starts inside its header or outside its buffer."
            );
        }
    }
    ensure!(!headers.is_empty(), "Clipboard HTML has no header.");
    ensure!(
        matches!(headers.get("Version"), Some(&"0.9" | &"1.0")) && text.starts_with("Version:"),
        "Clipboard HTML has an unsupported or missing version."
    );
    let field = |key: &str| {
        headers
            .get(key)
            .copied()
            .with_context(|| format!("Clipboard HTML has no {key} header"))
    };
    let start = offset(field("StartFragment")?)?;
    let end = offset(field("EndFragment")?)?;
    ensure!(
        cursor <= start && start <= end && end <= text.len(),
        "Clipboard HTML has an invalid fragment range."
    );
    ensure!(
        text.is_char_boundary(start) && text.is_char_boundary(end),
        "Clipboard HTML splits a UTF-8 character."
    );
    let selection = match (headers.get("StartSelection"), headers.get("EndSelection")) {
        (None, None) => None,
        (Some(selection_start), Some(selection_end)) => {
            let selection_start = offset(selection_start)?;
            let selection_end = offset(selection_end)?;
            ensure!(
                start <= selection_start
                    && selection_start <= selection_end
                    && selection_end <= end
                    && text.is_char_boundary(selection_start)
                    && text.is_char_boundary(selection_end),
                "Clipboard HTML has an invalid selection range."
            );
            Some((selection_start, selection_end))
        }
        _ => anyhow::bail!("Clipboard HTML has an incomplete selection range."),
    };
    let (selected_start, selected_end) = selection.unwrap_or((start, end));
    let fragment = &text[selected_start..selected_end];
    match (field("StartHTML")?, field("EndHTML")?) {
        ("-1", "-1") if selected_start == start => Ok(fragment.into()),
        ("-1", "-1") => with_ancestors(&text[start..selected_start], fragment),
        ("-1", _) | (_, "-1") => anyhow::bail!("Clipboard HTML has an incomplete context range."),
        (html_start, html_end) => {
            let html_start = offset(html_start)?;
            let html_end = offset(html_end)?;
            ensure!(
                cursor <= html_start
                    && html_start <= start
                    && end <= html_end
                    && html_end <= text.len()
                    && text.is_char_boundary(html_start)
                    && text.is_char_boundary(html_end),
                "Clipboard HTML has an invalid context range."
            );
            with_ancestors(&text[html_start..selected_start], fragment)
        }
    }
}

fn offset(value: &str) -> Result<usize> {
    ensure!(
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()),
        "Clipboard HTML has a malformed byte offset."
    );
    value
        .parse()
        .context("Clipboard HTML byte offset is too large")
}

fn with_ancestors(prefix: &str, fragment: &str) -> Result<String> {
    const MARKER: &str = "exeditor-clipboard-fragment-boundary";
    ensure!(
        !prefix.contains(MARKER),
        "Clipboard HTML contains a reserved context marker."
    );
    // Parse only the preceding context: sibling text is never serialized into the paste.
    let context = Html::parse_document(&format!("{prefix}<!--{MARKER}-->"));
    let boundary = context
        .tree
        .nodes()
        .find(|node| {
            node.value()
                .as_comment()
                .is_some_and(|comment| comment.comment.as_ref() == MARKER)
        })
        .context("Clipboard HTML starts inside a tag, comment, or raw-text element")?;
    let ancestors: Vec<_> = boundary.ancestors().filter_map(ElementRef::wrap).collect();
    ensure!(
        ancestors.len() <= 128,
        "Clipboard HTML is nested too deeply to paste safely."
    );
    let mut output = String::new();
    let mut closing = Vec::new();
    for ancestor in ancestors.into_iter().rev() {
        let name = ancestor.value().name();
        if matches!(name, "head" | "script" | "style" | "noscript" | "template") {
            return Ok(String::new());
        }
        // shortcut: retain supported inline/structural context, add CSS cascade support only with a richer importer.
        let name = match name {
            "html" | "body" => "div",
            "div" | "section" | "article" | "main" | "blockquote" | "p" | "pre" | "h1" | "h2"
            | "h3" | "h4" | "h5" | "h6" | "li" | "ul" | "ol" | "table" | "thead" | "tbody"
            | "tfoot" | "tr" | "td" | "th" | "span" | "font" | "b" | "strong" | "i" | "em"
            | "u" | "s" | "strike" | "del" | "sup" | "sub" | "a" | "code" => name,
            _ => continue,
        };
        output.push('<');
        output.push_str(name);
        for (attribute, value) in ancestor.value().attrs() {
            if matches!(
                attribute,
                "style" | "face" | "color" | "align" | "href" | "colspan" | "rowspan"
            ) {
                output.push(' ');
                output.push_str(attribute);
                output.push_str("=\"");
                output.push_str(
                    &value
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('"', "&quot;"),
                );
                output.push('"');
            }
        }
        output.push('>');
        closing.push(name);
    }
    output.push_str(fragment);
    for name in closing.into_iter().rev() {
        output.push_str("</");
        output.push_str(name);
        output.push('>');
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(before: &str, fragment: &str, after: &str, context: bool) -> Vec<u8> {
        let header = "Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
        let start = header.len();
        let fragment_start = start + before.len();
        let fragment_end = fragment_start + fragment.len();
        let end = fragment_end + after.len();
        let html_start = if context {
            format!("{start:010}")
        } else {
            "-1".into()
        };
        let html_end = if context {
            format!("{end:010}")
        } else {
            "-1".into()
        };
        // Keep absent-context fields the same width so byte offsets remain absolute.
        format!("Version:1.0\r\nStartHTML:{html_start:>10}\r\nEndHTML:{html_end:>10}\r\nStartFragment:{fragment_start:010}\r\nEndFragment:{fragment_end:010}\r\n{before}{fragment}{after}").into_bytes()
    }

    #[test]
    fn unicode_offsets_and_ancestor_styles_exclude_sibling_text() {
        let data = payload(
            "<html><body><p style='color:#123456'>outside 🦀 <b>",
            "café 😀",
            "</b> more outside</p></body></html>",
            true,
        );
        let html = decode(&data).expect("valid UTF-8 byte offsets");
        assert!(html.contains("<b>café 😀</b>"));
        assert!(html.contains("color:#123456"));
        assert!(!html.contains("outside"));
        assert!(!html.contains('🦀'));
    }

    #[test]
    fn fragment_only_and_nul_padding_are_supported() {
        let mut data = payload("", "<i>selected 🦀</i>", "", false);
        data.extend_from_slice(&[0, 0, 0]);
        assert_eq!(decode(&data).expect("no context"), "<i>selected 🦀</i>");
    }

    #[test]
    fn no_context_markers_are_not_parsed_as_headers_or_pasted() {
        let data = payload(
            "<!--StartFragment-->",
            "<b>selected</b>",
            "<!--EndFragment-->",
            false,
        );
        assert_eq!(
            decode(&data).expect("marker prefix without context"),
            "<b>selected</b>"
        );
    }

    #[test]
    fn list_and_table_ancestors_survive_without_other_items() {
        for (before, after, expected) in [
            (
                "<html><body><ul><li>outside</li><li>",
                "</li><li>outside</li></ul></body></html>",
                "<ul><li>selected</li></ul>",
            ),
            (
                "<html><body><table><tr><td>outside</td><td>",
                "</td></tr></table></body></html>",
                "<table><tbody><tr><td>selected</td></tr></tbody></table>",
            ),
        ] {
            let html =
                decode(&payload(before, "selected", after, true)).expect("structural context");
            assert!(html.contains(expected), "{html}");
            assert!(!html.contains("outside"));
        }
    }

    #[test]
    fn all_header_line_endings_are_supported() {
        for separator in ["\r", "\n", "\r\n"] {
            let template = format!(
                "Version:0.9{separator}StartHTML:-1{separator}EndHTML:-1{separator}StartFragment:0000000000{separator}EndFragment:0000000000{separator}"
            );
            let start = template.len();
            let header = template
                .replacen(
                    "StartFragment:0000000000",
                    &format!("StartFragment:{start:010}"),
                    1,
                )
                .replacen(
                    "EndFragment:0000000000",
                    &format!("EndFragment:{:010}", start + 1),
                    1,
                );
            assert_eq!(
                decode(format!("{header}x").as_bytes()).expect("header line ending"),
                "x"
            );
        }
    }

    #[test]
    fn malformed_ranges_headers_and_utf8_are_rejected() {
        let valid = payload("", "😀", "", false);
        let text = String::from_utf8(valid.clone()).expect("fixture Unicode");
        let start = valid.len() - "😀".len();
        for malformed in [
            text.replace("Version:1.0", "Version:2.0"),
            text.replace("StartHTML:        -1", "StartHTML:        -2"),
            text.replace("EndHTML:        -1", "EndHTML:0000000000"),
            text.replace("StartFragment:", "StartFragment:+"),
            text.replace("EndFragment:", "StartFragment:"),
            text.replace("EndFragment:", "EndSelection:"),
            text.replace(
                &format!("StartFragment:{start:010}"),
                "StartFragment:0000000000",
            ),
        ] {
            assert!(
                decode(malformed.as_bytes()).is_err(),
                "accepted {malformed:?}"
            );
        }
        let split = text.replace(
            &format!("StartFragment:{start:010}"),
            &format!("StartFragment:{:010}", start + 1),
        );
        assert!(decode(split.as_bytes()).is_err());
        let mut bad_utf8 = valid;
        *bad_utf8.last_mut().expect("nonempty fixture") = 255;
        assert!(decode(&bad_utf8).is_err());
        assert!(decode(&vec![b'x'; MAX_BYTES + 1]).is_err());
    }

    #[test]
    fn optional_selection_limits_paste_to_exact_copied_text_with_formatting() {
        for context in [true, false] {
            let template = "Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\nStartSelection:0000000000\r\nEndSelection:0000000000\r\n";
            let body = "<p>outside <b>selected \u{1f600}</b> outside</p>";
            let start = template.len();
            let end = start + body.len();
            let selection_start = start + body.find("selected").expect("selection");
            let selection_end = selection_start + "selected \u{1f600}".len();
            let html_start = if context {
                format!("{start:010}")
            } else {
                format!("{:>10}", "-1")
            };
            let html_end = if context {
                format!("{end:010}")
            } else {
                format!("{:>10}", "-1")
            };
            let payload = format!(
                "Version:1.0\r\nStartHTML:{html_start}\r\nEndHTML:{html_end}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\nStartSelection:{selection_start:010}\r\nEndSelection:{selection_end:010}\r\n{body}"
            );
            let html = decode(payload.as_bytes()).expect("precise selection");
            assert!(html.contains("<b>selected \u{1f600}</b>"), "{html}");
            assert!(!html.contains("outside"), "{html}");
        }
    }
    #[test]
    fn offsets_do_not_fall_back_to_fragment_comments() {
        let data = payload(
            "<html><body><!--StartFragment--><p>outside</p>",
            "<b>selected</b>",
            "<p>outside</p><!--EndFragment--></body></html>",
            true,
        );
        let html = decode(&data).expect("explicit byte ranges win");
        assert!(html.contains("selected"));
        assert!(!html.contains("outside"));
    }
}
