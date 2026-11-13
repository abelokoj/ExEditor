//! UTF-16 application coordinates mapped to NSH's structural document positions.
use anyhow::{Result, ensure};
use nsh_base::rich_editor::model::Node;
use std::ops::Range;

pub(super) struct TextMap {
    pub text: String,
    positions: Vec<Option<usize>>,
    started: bool,
}
impl TextMap {
    pub fn new(doc: &Node) -> Self {
        let mut map = Self {
            text: String::new(),
            positions: vec![Some(1)],
            started: false,
        };
        map.blocks(doc, 0, false);
        map
    }
    fn separator(&mut self, separator: char) {
        self.text.push(separator);
        self.positions.push(None);
    }
    fn blocks(&mut self, node: &Node, start: usize, cell: bool) {
        if node.is_textblock() {
            if self.positions.last().is_some_and(Option::is_some) && self.started {
                self.separator(if cell { ' ' } else { '\n' });
            }
            self.started = true;
            *self
                .positions
                .last_mut()
                .expect("the map has an initial boundary") = Some(start + 1);
            self.inline(node, start + 1);
            return;
        }
        let row = node.ty().name() == "tableRow";
        if row && self.started && self.positions.last().is_some_and(Option::is_some) {
            self.separator('\n');
        }
        let mut pos = if node.ty().name() == "doc" {
            start
        } else {
            start + 1
        };
        for index in 0..node.child_count() {
            let child = node.child(index);
            if row && index > 0 {
                self.separator('\t');
            }
            self.blocks(child, pos, cell || row);
            pos += child.node_size();
        }
    }
    fn inline(&mut self, node: &Node, mut pos: usize) {
        for index in 0..node.child_count() {
            let child = node.child(index);
            if let Some(text) = child.text() {
                self.append(text, pos);
            } else if child.ty().name() == "hardBreak" {
                self.append("\n", pos);
            } else if !child.is_leaf() {
                self.inline(child, pos + 1);
            }
            pos += child.node_size();
        }
    }
    fn append(&mut self, text: &str, mut pos: usize) {
        for ch in text.chars() {
            self.text.push(ch);
            if ch.len_utf16() == 2 {
                self.positions.push(None);
            }
            pos += ch.len_utf16();
            self.positions.push(Some(pos));
        }
    }
    pub fn len(&self) -> usize {
        self.positions.len() - 1
    }
    pub fn position(&self, offset: usize) -> Result<usize> {
        self.positions
            .get(offset)
            .copied()
            .flatten()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "the UTF-16 position splits a character or lies outside the document"
                )
            })
    }
    pub fn offset(&self, pos: usize) -> usize {
        self.positions
            .iter()
            .enumerate()
            .filter_map(|(offset, p)| p.map(|p| (offset, p.abs_diff(pos))))
            .min_by_key(|(_, distance)| *distance)
            .map_or(0, |(offset, _)| offset)
    }
    pub fn slice(&self, range: Range<usize>) -> Result<&str> {
        ensure!(range.start <= range.end, "the selection is reversed");
        let start = utf16_to_byte(&self.text, range.start).ok_or_else(|| {
            anyhow::anyhow!("the selection starts inside a character or outside the document")
        })?;
        let end = utf16_to_byte(&self.text, range.end).ok_or_else(|| {
            anyhow::anyhow!("the selection ends inside a character or outside the document")
        })?;
        Ok(&self.text[start..end])
    }
}
pub(super) fn utf16_to_byte(text: &str, offset: usize) -> Option<usize> {
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
