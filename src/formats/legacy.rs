//! Word 97–2007 binary main-story text extraction, based on the MS-DOC piece table.
//! shortcut: binary DOC imports main-story text only; use DOCX/RTF for formatting and media fidelity.
use super::{MAX_HTML_BYTES, RichDocument};
use anyhow::{Context, Result, ensure};
use std::io::{Cursor, Read as _};

const MAX_STREAM_BYTES: u64 = 128 * 1024 * 1024;
fn u16_at(bytes: &[u8], offset: usize) -> Result<u16> {
    let bytes = bytes
        .get(offset..offset + 2)
        .context("Truncated legacy Word header")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}
fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let bytes = bytes
        .get(offset..offset + 4)
        .context("Truncated legacy Word structure")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
fn range(bytes: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    let end = start
        .checked_add(len)
        .context("Legacy Word range overflow")?;
    bytes
        .get(start..end)
        .context("Legacy Word text points outside its stream")
}
fn stream(compound: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>> {
    ensure!(
        compound.entry(name)?.len() <= MAX_STREAM_BYTES,
        "A legacy Word stream exceeds its size limit."
    );
    let mut bytes = Vec::new();
    compound
        .open_stream(name)?
        .take(MAX_STREAM_BYTES + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_STREAM_BYTES,
        "A legacy Word stream exceeds its expanded size limit."
    );
    Ok(bytes)
}
fn piece_text(word: &[u8], table: &[u8]) -> Result<String> {
    ensure!(
        u16_at(word, 0)? == 0xa5ec,
        "This is not a supported Word binary document."
    );
    ensure!(
        matches!(u16_at(word, 2)?, 0x00c1 | 0x00d9 | 0x0101 | 0x010c | 0x0112),
        "This legacy Word version is unsupported; open a Word 97–2007 DOC, DOCX or RTF document."
    );
    let flags = u16_at(word, 10)?;
    ensure!(
        flags & 0x8100 == 0,
        "Encrypted legacy Word documents cannot be opened."
    );
    let words = usize::from(u16_at(word, 32)?);
    let lw_count_pos = 34 + words * 2;
    let long_count = usize::from(u16_at(word, lw_count_pos)?);
    ensure!(
        long_count >= 4,
        "Legacy Word header has no main-story length."
    );
    let lw_start = lw_count_pos + 2;
    let characters = u32_at(word, lw_start + 3 * 4)? as usize;
    ensure!(
        characters <= MAX_HTML_BYTES / 2,
        "Legacy Word text exceeds its character limit."
    );
    let pairs_count_pos = lw_start + long_count * 4;
    ensure!(
        u16_at(word, pairs_count_pos)? > 33,
        "Legacy Word header has no piece table."
    );
    // MS-DOC FibRgFcLcb97 pair 33 is fcClx/lcbClx in every supported FIB version.
    let clx_pair = pairs_count_pos + 2 + 33 * 8;
    let start = u32_at(word, clx_pair)? as usize;
    let length = u32_at(word, clx_pair + 4)? as usize;
    let clx = range(table, start, length)?;
    let mut offset = 0;
    while clx.get(offset) == Some(&1) {
        let length = usize::from(u16_at(clx, offset + 1)?);
        offset = offset
            .checked_add(3 + length)
            .context("Legacy Word piece-table offset overflow")?;
        ensure!(
            offset <= clx.len(),
            "Truncated legacy Word formatting record."
        );
    }
    ensure!(
        clx.get(offset) == Some(&2),
        "Legacy Word document has no text piece table."
    );
    let length = u32_at(clx, offset + 1)? as usize;
    let plc = range(clx, offset + 5, length)?;
    ensure!(
        length >= 4 && (length - 4) % 12 == 0,
        "Invalid legacy Word piece-table size."
    );
    let pieces = (length - 4) / 12;
    ensure!(pieces <= 1_000_000, "Legacy Word has too many text pieces.");
    let mut units = Vec::new();
    let mut expected = 0;
    for index in 0..pieces {
        let from = u32_at(plc, index * 4)? as usize;
        let to = u32_at(plc, (index + 1) * 4)? as usize;
        ensure!(
            from == expected && to >= from,
            "Legacy Word text pieces are not ordered."
        );
        expected = to;
        if from >= characters {
            break;
        }
        let count = to.min(characters) - from;
        let pcd = (pieces + 1) * 4 + index * 8;
        let fc = u32_at(plc, pcd + 2)?;
        let compressed = fc & 0x4000_0000 != 0;
        let physical = (fc & 0x3fff_ffff) as usize;
        if compressed {
            let bytes = range(word, physical / 2, count)?;
            let text = encoding_rs::WINDOWS_1252.decode(bytes).0;
            units.extend(text.encode_utf16());
        } else {
            let bytes = range(
                word,
                physical,
                count
                    .checked_mul(2)
                    .context("Legacy Word text length overflow")?,
            )?;
            units.extend(
                bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])),
            );
        }
    }
    ensure!(
        units.len() == characters,
        "Legacy Word main-story text is incomplete."
    );
    let text = String::from_utf16(&units).context("Invalid Unicode in legacy Word text")?;
    let mut visible = String::new();
    let mut fields = Vec::new();
    for c in text.chars() {
        match c {
            '\u{13}' => {
                ensure!(
                    fields.len() < 128,
                    "Legacy Word fields are nested too deeply."
                );
                fields.push(false);
            }
            '\u{14}' => {
                if let Some(field) = fields.last_mut() {
                    *field = true;
                }
            }
            '\u{15}' => {
                fields.pop();
            }
            _ if fields.iter().all(|result| *result) => match c {
                '\r' => visible.push('\n'),
                '\u{7}' => visible.push('\t'),
                '\u{b}' | '\u{c}' => visible.push('\n'),
                '\t' | '\n' => visible.push(c),
                c if !c.is_control() && c != '\u{fffc}' => visible.push(c),
                _ => {}
            },
            _ => {}
        }
    }
    Ok(visible)
}
/// Binary DOC is an import-only recovery of main-story text, never an overwrite format.
pub(super) fn read(bytes: &[u8]) -> Result<RichDocument> {
    if bytes.trim_ascii_start().starts_with(b"{\\rtf") {
        return super::import_rtf(bytes);
    }
    let mut compound = cfb::CompoundFile::open(Cursor::new(bytes))
        .context("Opening the legacy Word compound file")?;
    ensure!(
        compound.walk().take(16_385).count() <= 16_384,
        "Legacy Word has too many compound-file entries."
    );
    let word = stream(&mut compound, "WordDocument")?;
    let flags = u16_at(&word, 10)?;
    ensure!(
        flags & 0x8100 == 0,
        "Encrypted legacy Word documents cannot be opened."
    );
    let table = stream(
        &mut compound,
        if flags & 0x0200 != 0 {
            "1Table"
        } else {
            "0Table"
        },
    )?;
    Ok(RichDocument::from_text(&piece_text(&word, &table)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    fn fixture(text: &str) -> Vec<u8> {
        let units: Vec<_> = text.encode_utf16().collect();
        let mut word = vec![0; 512];
        word[0..2].copy_from_slice(&0xa5ecu16.to_le_bytes());
        word[2..4].copy_from_slice(&0x00c1u16.to_le_bytes());
        word[32..34].copy_from_slice(&14u16.to_le_bytes());
        word[62..64].copy_from_slice(&22u16.to_le_bytes());
        word[76..80].copy_from_slice(&(units.len() as u32).to_le_bytes());
        word[152..154].copy_from_slice(&93u16.to_le_bytes());
        word[422..426].copy_from_slice(&21u32.to_le_bytes());
        for unit in units.iter() {
            word.extend_from_slice(&unit.to_le_bytes());
        }
        let mut table = vec![2];
        table.extend_from_slice(&16u32.to_le_bytes());
        table.extend_from_slice(&0u32.to_le_bytes());
        table.extend_from_slice(&(units.len() as u32).to_le_bytes());
        table.extend_from_slice(&0u16.to_le_bytes());
        table.extend_from_slice(&512u32.to_le_bytes());
        table.extend_from_slice(&0u16.to_le_bytes());
        let mut compound = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        compound
            .create_stream("WordDocument")
            .unwrap()
            .write_all(&word)
            .unwrap();
        compound
            .create_stream("0Table")
            .unwrap()
            .write_all(&table)
            .unwrap();
        compound.into_inner().into_inner()
    }
    #[test]
    fn binary_doc_extracts_unicode_main_story_and_field_results() {
        let bytes = fixture("café 😀\r\u{13}HYPERLINK hidden\u{14}visible\u{15}\r");
        let doc = read(&bytes).unwrap();
        assert!(doc.html.contains("café 😀"));
        assert!(doc.html.contains("visible"));
        assert!(!doc.html.contains("HYPERLINK"));
    }
    #[test]
    fn malformed_legacy_offsets_fail_without_flattening_arbitrary_binary() {
        assert!(read(b"not a Word document").is_err());
        let mut word = vec![0; 512];
        word[0..2].copy_from_slice(&0xa5ecu16.to_le_bytes());
        assert!(piece_text(&word, &[]).is_err());
    }
}
