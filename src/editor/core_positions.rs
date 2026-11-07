//! Maps the app's UTF-16 text coordinates to WordCraft's UTF-8 paragraph positions.
use anyhow::{Result, bail, ensure};
use std::ops::Range;
use wordcraft_doc::{Block, Blocks, Document, Path, Pos, StoryRef};

#[derive(Clone)]
struct Segment {
    units: Range<usize>,
    bytes: Range<usize>,
    start: Pos,
    end: Pos,
    direct: bool,
}
#[derive(Clone, Default)]
pub(super) struct TextMap {
    pub text: String,
    segments: Vec<Segment>,
    len: usize,
}
impl TextMap {
    pub fn new(doc: &Document) -> Self {
        let mut map = Self::blocks(&doc.body, &[], 0);
        map.trim_newlines(1);
        map
    }
    fn blocks(blocks: &Blocks, prefix: &[u32], depth: usize) -> Self {
        let mut out = Self::default();
        if depth > 18 {
            return out;
        }
        for (index, block) in blocks.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(index as u32);
            match block.as_ref() {
                Block::Para(p) => {
                    let pos = |off| Pos::new(StoryRef::Body, Path(path.clone()), off);
                    let mut at = 0;
                    let mut object = 0;
                    for (byte, ch) in p.text.char_indices() {
                        if ch == wordcraft_doc::para::OBJ {
                            if at < byte {
                                out.push(&p.text[at..byte], pos(at), pos(byte), true);
                            }
                            let shown = p.objects.get(object).map_or("", |o| o.plain_text());
                            out.push(shown, pos(byte), pos(byte + ch.len_utf8()), false);
                            object += 1;
                            at = byte + ch.len_utf8();
                        }
                    }
                    out.push(&p.text[at..], pos(at), pos(p.text.len()), true);
                    out.push("\n", pos(p.text.len()), pos(p.text.len()), false);
                }
                Block::Table(table) => {
                    for (row_index, row) in table.rows.iter().enumerate() {
                        for (cell_index, cell) in row.cells.iter().enumerate() {
                            if cell_index > 0 {
                                out.separator("\t");
                            }
                            let mut cell_path = path.clone();
                            cell_path.extend([row_index as u32, cell_index as u32]);
                            let mut cell = Self::blocks(&cell.blocks, &cell_path, depth + 1);
                            cell.trim_newlines(usize::MAX);
                            cell.text = cell.text.replace('\n', " ");
                            out.append(cell);
                        }
                        out.separator("\n");
                    }
                }
            }
        }
        out
    }
    fn push(&mut self, text: &str, start: Pos, end: Pos, direct: bool) {
        let units = text.encode_utf16().count();
        let bytes = self.text.len()..self.text.len() + text.len();
        self.text.push_str(text);
        self.segments.push(Segment {
            units: self.len..self.len + units,
            bytes,
            start,
            end,
            direct,
        });
        self.len += units;
    }
    fn separator(&mut self, text: &str) {
        let pos = self
            .segments
            .last()
            .map(|s| s.end.clone())
            .unwrap_or_else(|| Pos::body(0, 0));
        self.push(text, pos.clone(), pos, false);
    }
    fn append(&mut self, other: Self) {
        let bytes = self.text.len();
        self.text.push_str(&other.text);
        for mut segment in other.segments {
            segment.units = segment.units.start + self.len..segment.units.end + self.len;
            segment.bytes = segment.bytes.start + bytes..segment.bytes.end + bytes;
            self.segments.push(segment);
        }
        self.len += other.len;
    }
    fn trim_newlines(&mut self, limit: usize) {
        for _ in 0..limit {
            if !self.text.ends_with('\n') {
                break;
            }
            self.text.pop();
            self.len -= 1;
            if let Some(s) = self.segments.last_mut() {
                s.units.end = s.units.end.min(self.len);
                s.bytes.end = s.bytes.end.min(self.text.len());
                if s.units.is_empty() && s.start == s.end {
                    self.segments.pop();
                }
            }
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn position(&self, units: usize) -> Result<Pos> {
        ensure!(
            units <= self.len,
            "the requested position is outside the document"
        );
        for s in self.segments.iter().rev() {
            if units < s.units.start || units > s.units.end {
                continue;
            }
            if s.direct {
                let local = units - s.units.start;
                let byte = utf16_to_byte(&self.text[s.bytes.clone()], local)
                    .ok_or_else(|| anyhow::anyhow!("a position splits a Unicode character"))?;
                return Ok(Pos {
                    off: s.start.off + byte,
                    ..s.start.clone()
                });
            }
            if units == s.units.start {
                return Ok(s.start.clone());
            }
            if units == s.units.end {
                return Ok(s.end.clone());
            }
            bail!("the requested position is inside an embedded field");
        }
        Ok(Pos::body(0, 0))
    }
    pub fn offset(&self, pos: &Pos) -> Option<usize> {
        for s in &self.segments {
            if s.start.story != pos.story || s.start.path != pos.path {
                continue;
            }
            if s.direct && pos.off >= s.start.off && pos.off <= s.end.off {
                let local = pos.off - s.start.off;
                return self
                    .text
                    .get(s.bytes.start..s.bytes.start + local)
                    .map(|t| s.units.start + t.encode_utf16().count());
            }
            if pos == &s.start {
                return Some(s.units.start);
            }
            if pos == &s.end {
                return Some(s.units.end);
            }
        }
        None
    }
    pub fn slice(&self, range: Range<usize>) -> Result<&str> {
        ensure!(range.start <= range.end, "the selection is reversed");
        let start = utf16_to_byte(&self.text, range.start)
            .ok_or_else(|| anyhow::anyhow!("invalid selection start"))?;
        let end = utf16_to_byte(&self.text, range.end)
            .ok_or_else(|| anyhow::anyhow!("invalid selection end"))?;
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
