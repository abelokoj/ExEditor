//! Editing plans with rich-fragment provenance and RichEdit UTF-16 coordinates.
//! Original pieces retain their source formatting when assembled by the native editor.

use std::ops::Range;

use anyhow::{Context as _, Result, ensure};

/// An editing action applied to the lines touched by a document selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineAction {
    MoveUp,
    MoveDown,
    CopyUp,
    CopyDown,
    Delete,
    InsertAbove,
    InsertBelow,
    Indent,
    Outdent,
    ToggleComment,
    AddComment,
    RemoveComment,
    TrimTrailingWhitespace,
}

/// A replacement fragment copied from the original document or newly inserted text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditPiece {
    /// A UTF-16 range in the original document, retaining its rich formatting.
    Original(Range<usize>),
    /// New text inheriting the insertion point's formatting.
    Text(String),
}

/// One replacement and its resulting selection, using UTF-16 code units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub replacement: String,
    pub selection: Range<usize>,
    /// Assembly instructions producing exactly `replacement` from the original document.
    pub pieces: Vec<EditPiece>,
}

#[derive(Clone, Debug, Default)]
struct LinePieces {
    content: Vec<EditPiece>,
    newline: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug)]
struct ColumnEdit {
    start: usize,
    removed: usize,
    inserted: usize,
}

impl ColumnEdit {
    fn map(self, column: usize) -> usize {
        if column < self.start {
            column
        } else if column <= self.start + self.removed {
            self.start + self.inserted
        } else {
            column - self.removed + self.inserted
        }
    }
}

/// Plans a line operation without mutating the document or invoking native controls.
/// A selection ending at the next line's start excludes that next line. Trimming
/// applies to selected lines, or the entire document when the selection is empty.
/// Moves leave a terminal newline in place rather than moving it as an empty line.
///
/// # Errors
/// Rejects non-LF input, invalid UTF-16 selection boundaries, and invalid comment prefixes.
pub fn plan(
    text: &str,
    selection: Range<usize>,
    action: LineAction,
    comment_prefix: &str,
) -> Result<Option<TextEdit>> {
    validate_selection(text, &selection)?;
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let starts = line_starts(&lines);
    let mut provenance: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let start = starts[index];
            let end = start + utf16_len(line);
            LinePieces {
                content: if start == end {
                    Vec::new()
                } else {
                    vec![EditPiece::Original(start..end)]
                },
                newline: (index + 1 < lines.len()).then_some(end..end + 1),
            }
        })
        .collect();
    let (first, last) = selected_lines(&starts, &selection);
    let start_line = line_at(&starts, selection.start);
    let end_line = line_at(&starts, selection.end);
    let start_column = selection.start - starts[start_line];
    let end_column = selection.end - starts[end_line];
    let relative = selection.start - starts[first]..selection.end - starts[first];
    let had_final_newline = text.ends_with('\n');

    let next_selection = match action {
        LineAction::MoveUp | LineAction::MoveDown => {
            let movable = lines.len() - usize::from(had_final_newline);
            if first >= movable {
                return Ok(None);
            }
            let last = last.min(movable - 1);
            let new_first = if action == LineAction::MoveUp {
                if first == 0 {
                    return Ok(None);
                }
                lines[first - 1..=last].rotate_left(1);
                provenance[first - 1..=last].rotate_left(1);
                first - 1
            } else {
                if last + 1 >= movable {
                    return Ok(None);
                }
                lines[first..=last + 1].rotate_right(1);
                provenance[first..=last + 1].rotate_right(1);
                first + 1
            };
            block_selection(&lines, new_first, last - first + 1, relative)
        }
        LineAction::CopyUp | LineAction::CopyDown => {
            let copies = lines[first..=last].to_vec();
            let copied_pieces = provenance[first..=last].to_vec();
            let insertion = if action == LineAction::CopyUp {
                first
            } else {
                last + 1
            };
            drop(lines.splice(insertion..insertion, copies));
            drop(provenance.splice(insertion..insertion, copied_pieces));
            block_selection(&lines, insertion, last - first + 1, relative)
        }
        LineAction::Delete => {
            drop(lines.drain(first..=last));
            drop(provenance.drain(first..=last));
            if lines.is_empty() {
                lines.push(String::new());
                provenance.push(LinePieces::default());
            }
            let line = first.min(lines.len() - 1);
            let caret = line_starts(&lines)[line] + start_column.min(utf16_len(&lines[line]));
            caret..caret
        }
        LineAction::InsertAbove | LineAction::InsertBelow => {
            let source = if action == LineAction::InsertAbove {
                first
            } else {
                last
            };
            let indentation = &lines[source][..indentation_bytes(&lines[source])];
            let indentation = indentation.to_owned();
            let insertion = if action == LineAction::InsertAbove {
                first
            } else {
                last + 1
            };
            let column = utf16_len(&indentation);
            provenance.insert(
                insertion,
                LinePieces {
                    content: vec![EditPiece::Text(indentation.clone())],
                    newline: None,
                },
            );
            lines.insert(insertion, indentation);
            let caret = line_starts(&lines)[insertion] + column;
            caret..caret
        }
        _ => {
            let mut changes = vec![None; lines.len()];
            let (first, last) =
                if action == LineAction::TrimTrailingWhitespace && selection.is_empty() {
                    (0, lines.len() - 1)
                } else {
                    (first, last)
                };
            let comment_action = matches!(
                action,
                LineAction::ToggleComment | LineAction::AddComment | LineAction::RemoveComment
            );
            let prefix = comment_prefix.trim();
            if comment_action {
                ensure!(
                    !prefix.is_empty() && !comment_prefix.contains(['\n', '\r', '\0']),
                    "a line-comment prefix must be nonempty and contain no line breaks or NULs"
                );
            }
            let any_nonblank = lines[first..=last]
                .iter()
                .any(|line| !line.trim().is_empty());
            let remove_comments = action == LineAction::RemoveComment
                || (action == LineAction::ToggleComment
                    && any_nonblank
                    && lines[first..=last]
                        .iter()
                        .filter(|line| !line.trim().is_empty())
                        .all(|line| line[indentation_bytes(line)..].starts_with(prefix)));
            for index in first..=last {
                let line = &mut lines[index];
                let change = match action {
                    LineAction::Indent => {
                        line.insert_str(0, "    ");
                        Some(ColumnEdit {
                            start: 0,
                            removed: 0,
                            inserted: 4,
                        })
                    }
                    LineAction::Outdent => {
                        let removed = if line.starts_with('\t') {
                            1
                        } else {
                            line.bytes()
                                .take(4)
                                .take_while(|byte| *byte == b' ')
                                .count()
                        };
                        if removed == 0 {
                            None
                        } else {
                            line.replace_range(..removed, "");
                            Some(ColumnEdit {
                                start: 0,
                                removed,
                                inserted: 0,
                            })
                        }
                    }
                    LineAction::TrimTrailingWhitespace => {
                        let keep = line.trim_end_matches([' ', '\t']).len();
                        if keep == line.len() {
                            None
                        } else {
                            let start = utf16_len(&line[..keep]);
                            let removed = utf16_len(&line[keep..]);
                            line.truncate(keep);
                            Some(ColumnEdit {
                                start,
                                removed,
                                inserted: 0,
                            })
                        }
                    }
                    LineAction::ToggleComment
                    | LineAction::AddComment
                    | LineAction::RemoveComment => {
                        if first != last && any_nonblank && line.trim().is_empty() {
                            continue;
                        }
                        let byte = indentation_bytes(line);
                        let start = utf16_len(&line[..byte]);
                        if remove_comments {
                            if line[byte..].starts_with(prefix) {
                                let mut end = byte + prefix.len();
                                if line[end..].starts_with(' ') {
                                    end += 1;
                                }
                                let removed = utf16_len(&line[byte..end]);
                                line.replace_range(byte..end, "");
                                Some(ColumnEdit {
                                    start,
                                    removed,
                                    inserted: 0,
                                })
                            } else {
                                None
                            }
                        } else {
                            let marker = format!("{prefix} ");
                            line.insert_str(byte, &marker);
                            Some(ColumnEdit {
                                start,
                                removed: 0,
                                inserted: utf16_len(&marker),
                            })
                        }
                    }
                    _ => None,
                };
                if let Some(change) = change {
                    let start = utf16_to_byte(line, change.start)
                        .context("invalid inserted-text boundary")?;
                    let end = utf16_to_byte(line, change.start + change.inserted)
                        .context("invalid inserted-text boundary")?;
                    replace_pieces(
                        &mut provenance[index].content,
                        change.start..change.start + change.removed,
                        &line[start..end],
                    )?;
                }
                changes[index] = change;
            }
            let starts = line_starts(&lines);
            let map_column = |line: usize, column: usize| {
                changes[line].map_or(column, |change| change.map(column))
            };
            starts[start_line] + map_column(start_line, start_column)
                ..starts[end_line] + map_column(end_line, end_column)
        }
    };

    let mut replacement_document = lines.join("\n");
    let mut pieces = join_pieces(&provenance);
    if had_final_newline
        && !replacement_document.is_empty()
        && !replacement_document.ends_with('\n')
    {
        replacement_document.push('\n');
        if let Some(newline) = provenance.last().and_then(|line| line.newline.clone()) {
            append_piece(&mut pieces, EditPiece::Original(newline));
        } else {
            append_piece(&mut pieces, EditPiece::Text("\n".to_owned()));
        }
    }
    let total = utf16_len(&replacement_document);
    let next_selection = next_selection.start.min(total)..next_selection.end.min(total);
    let (prefix, _) = unchanged_edges(&pieces, utf16_len(text), total);
    if prefix == total && total == utf16_len(text) && selection == next_selection {
        return Ok(None);
    }
    Ok(Some(minimal_edit(
        text,
        &replacement_document,
        next_selection,
        pieces,
    )?))
}

/// Expands a selection to the complete touched lines, including their line endings.
///
/// # Errors
/// Rejects non-LF text or selections that split a Unicode scalar's UTF-16 encoding.
pub fn select_line(text: &str, selection: Range<usize>) -> Result<Range<usize>> {
    validate_selection(text, &selection)?;
    let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let starts = line_starts(&lines);
    let (first, last) = selected_lines(&starts, &selection);
    Ok(starts[first]
        ..starts
            .get(last + 1)
            .copied()
            .unwrap_or_else(|| utf16_len(text)))
}

/// Wraps selected text in block delimiters, or removes exact enclosing delimiters.
/// An empty selection targets its current line's content without the line ending.
/// Delimiters immediately surrounding the selection also toggle off, allowing a
/// repeated shortcut to unwrap the inner selection returned by a previous wrap.
/// Existing content remains an original rich fragment and is selected after either action.
///
/// # Errors
/// Rejects invalid UTF-16 positions, non-LF text, and empty or multiline delimiters.
pub fn toggle_block_comment(
    text: &str,
    selection: Range<usize>,
    open: &str,
    close: &str,
) -> Result<Option<TextEdit>> {
    validate_selection(text, &selection)?;
    ensure!(
        !open.is_empty()
            && !close.is_empty()
            && !open.contains(['\n', '\r', '\0'])
            && !close.contains(['\n', '\r', '\0']),
        "block-comment delimiters must be nonempty and contain no line breaks or NULs"
    );
    let selected = if selection.is_empty() {
        let mut line = select_line(text, selection)?;
        let end = utf16_to_byte(text, line.end).context("invalid line-end position")?;
        if line.end > line.start && text[..end].ends_with('\n') {
            line.end -= 1;
        }
        line
    } else {
        selection
    };
    let start_byte = utf16_to_byte(text, selected.start).context("invalid block-comment start")?;
    let end_byte = utf16_to_byte(text, selected.end).context("invalid block-comment end")?;
    let content = &text[start_byte..end_byte];
    let open_len = utf16_len(open);
    let close_len = utf16_len(close);
    let enclosing = if content.len() >= open.len() + close.len()
        && content.starts_with(open)
        && content.ends_with(close)
    {
        Some((
            selected.clone(),
            selected.start + open_len..selected.end - close_len,
        ))
    } else if text[..start_byte].ends_with(open) && text[end_byte..].starts_with(close) {
        Some((
            selected.start - open_len..selected.end + close_len,
            selected.clone(),
        ))
    } else {
        None
    };
    if let Some((range, inner)) = enclosing {
        let start = utf16_to_byte(text, inner.start).context("invalid inner comment start")?;
        let end = utf16_to_byte(text, inner.end).context("invalid inner comment end")?;
        let length = inner.end - inner.start;
        let mut pieces = Vec::new();
        append_piece(&mut pieces, EditPiece::Original(inner));
        let selection = range.start..range.start + length;
        return Ok(Some(TextEdit {
            range,
            replacement: text[start..end].to_owned(),
            selection,
            pieces,
        }));
    }
    let mut pieces = Vec::new();
    append_piece(&mut pieces, EditPiece::Text(open.to_owned()));
    append_piece(&mut pieces, EditPiece::Original(selected.clone()));
    append_piece(&mut pieces, EditPiece::Text(close.to_owned()));
    let start = selected.start + open_len;
    let selection = start..start + selected.end - selected.start;
    Ok(Some(TextEdit {
        range: selected,
        replacement: format!("{open}{content}{close}"),
        selection,
        pieces,
    }))
}

/// Finds the matching (), [], or {} bracket at or immediately before a UTF-16 caret.
/// Matching is language-independent and includes brackets in quoted text and comments.
/// Invalid Unicode positions and unbalanced or incorrectly nested brackets return None.
pub fn matching_bracket(text: &str, caret: usize) -> Option<usize> {
    let caret_byte = utf16_to_byte(text, caret)?;
    let current = text[caret_byte..]
        .chars()
        .next()
        .filter(|ch| bracket_pair(*ch).is_some());
    let (byte, target) = if let Some(current) = current {
        (caret_byte, current)
    } else {
        text[..caret_byte]
            .char_indices()
            .next_back()
            .filter(|(_, ch)| bracket_pair(*ch).is_some())?
    };
    let mut stack = Vec::new();
    if is_opening(target) {
        for (relative, ch) in text[byte..].char_indices() {
            if is_opening(ch) {
                stack.push(bracket_pair(ch)?);
            } else if matches!(ch, ')' | ']' | '}') {
                if stack.pop() != Some(ch) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(utf16_len(&text[..byte + relative]));
                }
            }
        }
    } else {
        for (position, ch) in text[..byte + target.len_utf8()].char_indices().rev() {
            if matches!(ch, ')' | ']' | '}') {
                stack.push(bracket_pair(ch)?);
            } else if is_opening(ch) {
                if stack.pop() != Some(ch) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(utf16_len(&text[..position]));
                }
            }
        }
    }
    None
}

fn bracket_pair(ch: char) -> Option<char> {
    match ch {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

fn is_opening(ch: char) -> bool {
    matches!(ch, '(' | '[' | '{')
}

fn validate_selection(text: &str, selection: &Range<usize>) -> Result<()> {
    ensure!(
        !text.contains('\r'),
        "line editing requires LF-normalized text"
    );
    ensure!(selection.start <= selection.end, "selection is reversed");
    utf16_to_byte(text, selection.start)
        .context("selection start is outside the document or splits a surrogate pair")?;
    utf16_to_byte(text, selection.end)
        .context("selection end is outside the document or splits a surrogate pair")?;
    Ok(())
}

fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn line_starts(lines: &[String]) -> Vec<usize> {
    let mut offset = 0;
    lines
        .iter()
        .map(|line| {
            let start = offset;
            offset += utf16_len(line) + 1;
            start
        })
        .collect()
}

fn line_at(starts: &[usize], offset: usize) -> usize {
    starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1)
}

fn selected_lines(starts: &[usize], selection: &Range<usize>) -> (usize, usize) {
    let first = line_at(starts, selection.start);
    let end = line_at(starts, selection.end);
    let last = if !selection.is_empty() && selection.end == starts[end] {
        end.saturating_sub(1)
    } else {
        end
    };
    (first, last)
}

fn block_selection(
    lines: &[String],
    first: usize,
    count: usize,
    relative: Range<usize>,
) -> Range<usize> {
    let starts = line_starts(lines);
    let start = starts[first];
    let end = starts
        .get(first + count)
        .copied()
        .unwrap_or_else(|| starts[lines.len() - 1] + utf16_len(&lines[lines.len() - 1]));
    (start + relative.start).min(end)..(start + relative.end).min(end)
}

fn indentation_bytes(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn piece_len(piece: &EditPiece) -> usize {
    match piece {
        EditPiece::Original(range) => range.end - range.start,
        EditPiece::Text(text) => utf16_len(text),
    }
}

fn append_piece(pieces: &mut Vec<EditPiece>, piece: EditPiece) {
    if piece_len(&piece) == 0 {
        return;
    }
    match (pieces.last_mut(), &piece) {
        (Some(EditPiece::Original(previous)), EditPiece::Original(next))
            if previous.end == next.start =>
        {
            previous.end = next.end;
            return;
        }
        (Some(EditPiece::Text(previous)), EditPiece::Text(next)) => {
            previous.push_str(next);
            return;
        }
        _ => {}
    }
    pieces.push(piece);
}

fn slice_pieces(pieces: &[EditPiece], range: Range<usize>) -> Result<Vec<EditPiece>> {
    ensure!(
        range.start <= range.end,
        "replacement-piece range is reversed"
    );
    let total: usize = pieces.iter().map(piece_len).sum();
    ensure!(
        range.end <= total,
        "replacement-piece range exceeds its content"
    );
    let mut result = Vec::new();
    let mut offset = 0;
    for piece in pieces {
        let end = offset + piece_len(piece);
        if range.start < end && offset < range.end {
            let local = range.start.saturating_sub(offset)..range.end.min(end) - offset;
            let sliced = match piece {
                EditPiece::Original(original) => {
                    EditPiece::Original(original.start + local.start..original.start + local.end)
                }
                EditPiece::Text(text) => {
                    let start = utf16_to_byte(text, local.start)
                        .context("piece starts inside a surrogate pair")?;
                    let end = utf16_to_byte(text, local.end)
                        .context("piece ends inside a surrogate pair")?;
                    EditPiece::Text(text[start..end].to_owned())
                }
            };
            append_piece(&mut result, sliced);
        }
        offset = end;
        if offset >= range.end {
            break;
        }
    }
    Ok(result)
}

fn replace_pieces(pieces: &mut Vec<EditPiece>, range: Range<usize>, text: &str) -> Result<()> {
    let total = pieces.iter().map(piece_len).sum();
    let mut replaced = slice_pieces(pieces, 0..range.start)?;
    append_piece(&mut replaced, EditPiece::Text(text.to_owned()));
    for piece in slice_pieces(pieces, range.end..total)? {
        append_piece(&mut replaced, piece);
    }
    *pieces = replaced;
    Ok(())
}

fn join_pieces(lines: &[LinePieces]) -> Vec<EditPiece> {
    let mut result = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        for piece in &line.content {
            append_piece(&mut result, piece.clone());
        }
        if index + 1 < lines.len() {
            let newline = line
                .newline
                .clone()
                .map_or_else(|| EditPiece::Text("\n".to_owned()), EditPiece::Original);
            append_piece(&mut result, newline);
        }
    }
    result
}

fn unchanged_edges(pieces: &[EditPiece], before_len: usize, after_len: usize) -> (usize, usize) {
    let mut prefix = 0;
    for piece in pieces {
        match piece {
            EditPiece::Original(range) if range.start == prefix => prefix = range.end,
            _ => break,
        }
    }
    let limit = (before_len - prefix).min(after_len - prefix);
    let mut suffix = 0;
    for piece in pieces.iter().rev() {
        match piece {
            EditPiece::Original(range)
                if range.end == before_len - suffix && piece_len(piece) <= limit - suffix =>
            {
                suffix += piece_len(piece);
            }
            _ => break,
        }
    }
    (prefix, suffix)
}

fn minimal_edit(
    before: &str,
    after: &str,
    selection: Range<usize>,
    pieces: Vec<EditPiece>,
) -> Result<TextEdit> {
    let before_len = utf16_len(before);
    let after_len = utf16_len(after);
    let (prefix, suffix) = unchanged_edges(&pieces, before_len, after_len);
    let start =
        utf16_to_byte(after, prefix).context("replacement starts inside a surrogate pair")?;
    let end = utf16_to_byte(after, after_len - suffix)
        .context("replacement ends inside a surrogate pair")?;
    Ok(TextEdit {
        range: prefix..before_len - suffix,
        replacement: after[start..end].to_owned(),
        selection,
        pieces: slice_pieces(&pieces, prefix..after_len - suffix)?,
    })
}

#[cfg(test)]
mod tests {
    use super::toggle_block_comment;
    use super::{
        EditPiece, LineAction, TextEdit, matching_bracket, plan, select_line, utf16_to_byte,
    };
    use std::ops::Range;

    fn materialize(text: &str, pieces: &[EditPiece]) -> String {
        let mut result = String::new();
        for piece in pieces {
            match piece {
                EditPiece::Original(range) => {
                    let start =
                        utf16_to_byte(text, range.start).expect("valid original-piece start");
                    let end = utf16_to_byte(text, range.end).expect("valid original-piece end");
                    result.push_str(&text[start..end]);
                }
                EditPiece::Text(inserted) => result.push_str(inserted),
            }
        }
        result
    }

    fn apply(text: &str, edit: &TextEdit) -> String {
        assert_eq!(
            materialize(text, &edit.pieces),
            edit.replacement,
            "rich-fragment assembly must match replacement"
        );
        let start = utf16_to_byte(text, edit.range.start).expect("valid planned start");
        let end = utf16_to_byte(text, edit.range.end).expect("valid planned end");
        let mut result = text.to_owned();
        result.replace_range(start..end, &edit.replacement);
        result
    }

    fn planned(text: &str, selection: Range<usize>, action: LineAction) -> (String, Range<usize>) {
        let edit = plan(text, selection, action, "//")
            .expect("valid input")
            .expect("an edit");
        (apply(text, &edit), edit.selection)
    }

    #[test]
    fn move_at_eof_does_not_add_a_line_ending() {
        assert_eq!(
            planned("one\ntwo", 4..7, LineAction::MoveUp),
            ("two\none".to_owned(), 0..3)
        );
        assert_eq!(
            planned("one\ntwo", 0..4, LineAction::MoveDown),
            ("two\none".to_owned(), 4..7)
        );
    }

    #[test]
    fn move_preserves_terminal_newline_and_excludes_next_line_start() {
        assert_eq!(
            planned("one\ntwo\n", 0..4, LineAction::MoveDown),
            ("two\none\n".to_owned(), 4..8)
        );
        assert!(
            plan("a\n", 2..2, LineAction::MoveUp, "//")
                .expect("valid input")
                .is_none()
        );
        assert!(
            plan("a\nb", 2..3, LineAction::MoveDown, "//")
                .expect("valid input")
                .is_none()
        );
    }

    #[test]
    fn moving_multiple_lines_keeps_relative_selection() {
        assert_eq!(
            planned("a\nbb\nccc\nd", 2..9, LineAction::MoveUp),
            ("bb\nccc\na\nd".to_owned(), 0..7)
        );
    }

    #[test]
    fn copy_down_keeps_unicode_caret_and_copies_eof_without_extra_breaks() {
        assert_eq!(
            planned("a\n😀x\nz", 4..4, LineAction::CopyDown),
            ("a\n😀x\n😀x\nz".to_owned(), 8..8)
        );
        assert_eq!(
            planned("one\ntwo", 4..7, LineAction::CopyDown),
            ("one\ntwo\ntwo".to_owned(), 8..11)
        );
        assert_eq!(
            planned("a\nb\n", 2..3, LineAction::CopyUp),
            ("a\nb\nb\n".to_owned(), 2..3)
        );
    }

    #[test]
    fn delete_removes_only_selected_lines() {
        assert_eq!(
            planned("a\nbb\nccc", 2..5, LineAction::Delete),
            ("a\nccc".to_owned(), 2..2)
        );
        assert_eq!(
            planned("a\nb\n", 2..4, LineAction::Delete),
            ("a\n".to_owned(), 2..2)
        );
        assert_eq!(
            planned("a", 0..1, LineAction::Delete),
            (String::new(), 0..0)
        );
    }

    #[test]
    fn insert_above_and_below_carry_current_indentation() {
        assert_eq!(
            planned("  a\nb", 2..2, LineAction::InsertAbove),
            ("  \n  a\nb".to_owned(), 2..2)
        );
        assert_eq!(
            planned("  a\nb", 2..2, LineAction::InsertBelow),
            ("  a\n  \nb".to_owned(), 6..6)
        );
    }

    #[test]
    fn indentation_uses_four_spaces_and_outdent_accepts_tabs_or_fewer_spaces() {
        assert_eq!(
            planned("\talpha\n  beta\n\n", 0..14, LineAction::Outdent),
            ("alpha\nbeta\n\n".to_owned(), 0..11)
        );
        assert_eq!(
            planned("a", 0..0, LineAction::Indent),
            ("    a".to_owned(), 4..4)
        );
        assert_eq!(
            planned("   a", 3..3, LineAction::Outdent),
            ("a".to_owned(), 0..0)
        );
    }

    #[test]
    fn toggle_removes_unanimous_comments_but_adds_for_mixed_lines() {
        let text = "  // α\n  // β\n";
        let edit = plan(text, 0..14, LineAction::ToggleComment, "//")
            .expect("valid input")
            .expect("comment edit");
        assert_eq!(apply(text, &edit), "  α\n  β\n");
        assert_eq!(
            planned("// a\nb", 0..6, LineAction::ToggleComment).0,
            "// // a\n// b"
        );
    }

    #[test]
    fn comments_preserve_indentation_and_support_language_prefixes() {
        let edit = plan("  α\n  β\n", 0..8, LineAction::AddComment, "#")
            .expect("valid input")
            .expect("comment edit");
        assert_eq!(apply("  α\n  β\n", &edit), "  # α\n  # β\n");
        let edit = plan("\t-- value", 0..9, LineAction::RemoveComment, "--")
            .expect("valid input")
            .expect("comment edit");
        assert_eq!(apply("\t-- value", &edit), "\tvalue");
        assert!(plan("value", 0..0, LineAction::AddComment, "").is_err());
    }

    #[test]
    fn trim_targets_selected_lines_or_all_lines_with_a_caret() {
        let text = "a  \nb \nc  ";
        assert_eq!(
            planned(text, 4..7, LineAction::TrimTrailingWhitespace).0,
            "a  \nb\nc  "
        );
        assert_eq!(
            planned(text, 0..0, LineAction::TrimTrailingWhitespace).0,
            "a\nb\nc"
        );
    }

    #[test]
    fn line_selection_does_not_include_next_line_when_range_ends_at_its_start() {
        assert_eq!(select_line("a\nb\nc", 0..2).expect("valid selection"), 0..2);
        assert_eq!(select_line("a\nb\nc", 2..2).expect("valid selection"), 2..4);
        assert_eq!(select_line("a\n", 2..2).expect("valid EOF selection"), 2..2);
    }

    #[test]
    fn invalid_unicode_offsets_and_crlf_input_are_rejected() {
        assert!(plan("😀a", 1..1, LineAction::Delete, "//").is_err());
        assert!(plan("a", 0..2, LineAction::Delete, "//").is_err());
        assert!(plan("a\r\nb", 0..0, LineAction::Delete, "//").is_err());
    }

    #[test]
    fn bracket_matching_uses_utf16_and_rejects_incorrect_nesting() {
        assert_eq!(matching_bracket("😀(a[β])", 2), Some(7));
        assert_eq!(matching_bracket("😀(a[β])", 8), Some(2));
        assert_eq!(matching_bracket("😀(a[β])", 1), None);
        assert_eq!(matching_bracket("([)]", 0), None);
        assert_eq!(matching_bracket("(unclosed", 0), None);
    }

    #[test]
    fn identical_lines_still_move_original_rich_fragments() {
        let edit = plan("same\nsame", 0..4, LineAction::MoveDown, "//")
            .expect("valid input")
            .expect("a rich-fragment edit");
        assert_eq!(apply("same\nsame", &edit), "same\nsame");
        assert_eq!(edit.range, 0..9);
        assert_eq!(
            edit.pieces,
            vec![
                EditPiece::Original(5..9),
                EditPiece::Text("\n".to_owned()),
                EditPiece::Original(0..4),
            ]
        );
    }

    #[test]
    fn prefix_insertion_keeps_original_content_outside_the_edit() {
        let edit = plan("😀 value", 0..0, LineAction::Indent, "//")
            .expect("valid input")
            .expect("an indent edit");
        assert_eq!(apply("😀 value", &edit), "    😀 value");
        assert_eq!(edit.range, 0..0);
        assert_eq!(edit.pieces, vec![EditPiece::Text("    ".to_owned())]);
    }

    #[test]
    fn replacement_pieces_reconstruct_every_line_action_at_document_boundaries() {
        let actions = [
            LineAction::MoveUp,
            LineAction::MoveDown,
            LineAction::CopyUp,
            LineAction::CopyDown,
            LineAction::Delete,
            LineAction::InsertAbove,
            LineAction::InsertBelow,
            LineAction::Indent,
            LineAction::Outdent,
            LineAction::ToggleComment,
            LineAction::AddComment,
            LineAction::RemoveComment,
            LineAction::TrimTrailingWhitespace,
        ];
        for text in ["", "\n", "one", "one\ntwo", "one\ntwo\n", "😀a  \n  β\n\n"] {
            let total = text.encode_utf16().count();
            for selection in [0..0, 0..total, total..total] {
                for action in actions {
                    if let Some(edit) =
                        plan(text, selection.clone(), action, "//").expect("valid boundary fixture")
                    {
                        let _ = apply(text, &edit);
                    }
                }
            }
        }
    }

    #[test]
    fn block_comment_wrap_preserves_unicode_fragment_and_selects_content() {
        let text = "α😀β\nnext";
        let edit = toggle_block_comment(text, 1..3, "/*", "*/")
            .expect("valid Unicode selection")
            .expect("a block comment edit");
        assert_eq!(apply(text, &edit), "α/*😀*/β\nnext");
        assert_eq!(edit.selection, 3..5);
        assert_eq!(
            edit.pieces,
            vec![
                EditPiece::Text("/*".to_owned()),
                EditPiece::Original(1..3),
                EditPiece::Text("*/".to_owned()),
            ]
        );
    }

    #[test]
    fn block_comment_unwraps_a_selection_widened_to_include_delimiters() {
        let original = "α😀β\nnext";
        let wrapped = toggle_block_comment(original, 1..3, "/*", "*/")
            .expect("valid input")
            .expect("a wrap edit");
        let commented = apply(original, &wrapped);
        let widened = wrapped.selection.start - 2..wrapped.selection.end + 2;
        let edit = toggle_block_comment(&commented, widened, "/*", "*/")
            .expect("valid enclosing selection")
            .expect("an unwrap edit");
        assert_eq!(apply(&commented, &edit), original);
        assert_eq!(edit.selection, 1..3);
        assert_eq!(edit.pieces, vec![EditPiece::Original(3..5)]);
    }

    #[test]
    fn repeated_block_comment_shortcut_unwraps_the_returned_inner_selection() {
        let text = "😀 value";
        let wrapped = toggle_block_comment(text, 0..2, "/*", "*/")
            .expect("valid input")
            .expect("a wrap edit");
        let commented = apply(text, &wrapped);
        let edit = toggle_block_comment(&commented, wrapped.selection, "/*", "*/")
            .expect("valid inner selection")
            .expect("an unwrap edit");
        assert_eq!(apply(&commented, &edit), text);
        assert_eq!(edit.selection, 0..2);
    }

    #[test]
    fn empty_block_selection_wraps_line_content_but_keeps_the_newline_outside() {
        let text = "α\n  beta\nγ";
        let edit = toggle_block_comment(text, 4..4, "<!--", "-->")
            .expect("valid caret")
            .expect("a line wrap edit");
        assert_eq!(apply(text, &edit), "α\n<!--  beta-->\nγ");
        assert_eq!(edit.range, 2..8);
        assert_eq!(edit.selection, 6..12);
        assert_eq!(edit.pieces[1], EditPiece::Original(2..8));
    }

    #[test]
    fn block_comments_support_empty_content_and_validate_delimiters_and_offsets() {
        let edit = toggle_block_comment("", 0..0, "/*", "*/")
            .expect("valid empty input")
            .expect("an empty wrap edit");
        assert_eq!(apply("", &edit), "/**/");
        assert_eq!(edit.selection, 2..2);
        let unwrapped = toggle_block_comment("/**/", 2..2, "/*", "*/")
            .expect("valid inner caret")
            .expect("an empty unwrap edit");
        assert_eq!(apply("/**/", &unwrapped), "");
        assert!(unwrapped.pieces.is_empty());
        assert!(toggle_block_comment("😀", 1..1, "/*", "*/").is_err());
        assert!(toggle_block_comment("value", 0..5, "", "*/").is_err());
        assert!(toggle_block_comment("value", 0..5, "/*\n", "*/").is_err());
    }
}
