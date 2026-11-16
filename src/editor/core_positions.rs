//! UTF-16 application coordinates mapped to NSH's structural document positions.
use anyhow::{Result, ensure};
use nsh_base::rich_editor::model::Node;
use std::{ops::Range, sync::Arc};

pub(super) struct TextMap {
    pub text: String,
    positions: Vec<Option<usize>>,
    started: bool,
    by_position: Vec<(usize, usize)>,
    bytes: Vec<Option<usize>>,
    pub line_starts: Arc<Vec<usize>>,
}
impl TextMap {
    pub fn new(doc: &Node) -> Self {
        let mut map = Self {
            text: String::new(),
            positions: vec![Some(1)],
            started: false,
            by_position: Vec::new(),
            bytes: Vec::new(),
            line_starts: Arc::new(Vec::new()),
        };
        map.blocks(doc, 0, false);
        map.by_position = map
            .positions
            .iter()
            .enumerate()
            .filter_map(|(offset, pos)| pos.map(|pos| (pos, offset)))
            .collect();
        map.by_position.sort_unstable();
        map.bytes.push(Some(0));
        let mut lines = vec![map.position(0).unwrap_or(1)];
        let mut units = 0;
        for (byte, ch) in map.text.char_indices() {
            if ch.len_utf16() == 2 {
                map.bytes.push(None);
            }
            map.bytes.push(Some(byte + ch.len_utf8()));
            units += ch.len_utf16();
            if ch == '\n' {
                if let Ok(pos) = map.position(units) {
                    lines.push(pos);
                }
            }
        }
        map.line_starts = Arc::new(lines);
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
    #[cfg(test)]
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
        let index = self.by_position.partition_point(|&(p, _)| p < pos);
        let right = self.by_position.get(index).copied();
        let left = index.checked_sub(1).map(|previous| {
            let p = self.by_position[previous].0;
            self.by_position[self
                .by_position
                .partition_point(|&(candidate, _)| candidate < p)]
        });
        left.into_iter()
            .chain(right)
            .min_by_key(|&(p, offset)| (p.abs_diff(pos), offset))
            .map_or(0, |(_, offset)| offset)
    }
    pub fn slice(&self, range: Range<usize>) -> Result<&str> {
        ensure!(range.start <= range.end, "the selection is reversed");
        let start = self
            .bytes
            .get(range.start)
            .copied()
            .flatten()
            .ok_or_else(|| {
                anyhow::anyhow!("the selection starts inside a character or outside the document")
            })?;
        let end = self
            .bytes
            .get(range.end)
            .copied()
            .flatten()
            .ok_or_else(|| {
                anyhow::anyhow!("the selection ends inside a character or outside the document")
            })?;
        Ok(&self.text[start..end])
    }
}
