//! Structural reader for `elements.data`.
//!
//! Layout, as written by `elementdataman::save_data`:
//!
//! ```text
//! u32 version (low 16 bits = format version, high bits = 0x1000)
//! u32 export timestamp
//! lists     u32 item_size, u32 count, count * item_size bytes
//! markers   before fixed list slots, depending on the version:
//!           checksum  8 bytes (a slice of the MD5 digest the client verifies)
//!           exporter  u32 0x19e75edf, u32 len, len bytes, u32 time
//!           tag       u32 0xee35679f, u32 len, len bytes
//! talk      u32 count, count * talk_proc (variable length), up to EOF
//! ```
//!
//! [`ElementsFile::parse_with`] reads a file against a known marker table.
//! [`ElementsFile::parse_detect`] recognises segments by content for versions
//! without one; it cannot tell a checksum slot holding raw bytes from an
//! empty list, so known tables always take precedence.

use encoding_rs::GBK;
use serde::Serialize;

use super::format::{Marker, MarkerKind};

pub const TAG_EXPORTER: u32 = 0x19e7_5edf;
pub const TAG_HARDWARE: u32 = 0xee35_679f;

const EXPORTER_KEY: [u8; 4] = [0x5f, 0x6d, 0xe8, 0xc9];
const MAX_ITEM_SIZE: u64 = 1 << 20;
const MAX_TAG_LEN: usize = 1 << 16;
const TALK_PROC_HEADER: usize = 4 + 128 + 4; // id_talk, text[64], num_window
const TALK_OPTION_SIZE: usize = 4 + 128 + 4; // id, text[64], param

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    Header,
    List,
    Checksum,
    Exporter,
    Tag,
    Talk,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub offset: usize,
    pub size: usize,
    /// For markers: the list slot they precede.
    pub before: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ListBlock {
    pub header_offset: usize,
    pub data_offset: usize,
    pub item_size: usize,
    pub count: usize,
}

#[derive(Debug)]
pub struct ElementsFile {
    pub data: Vec<u8>,
    pub raw_version: u32,
    pub timestamp: u32,
    pub lists: Vec<ListBlock>,
    pub segments: Vec<Segment>,
    pub talk_count: u32,
    pub exporter: Option<String>,
}

#[derive(Debug)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte offset {})", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn i32_at(data: &[u8], offset: usize) -> Option<i32> {
    u32_at(data, offset).map(|v| v as i32)
}

fn is_hex_checksum(data: &[u8], offset: usize) -> bool {
    data.get(offset..offset + 8)
        .is_some_and(|b| b.iter().all(u8::is_ascii_hexdigit))
}

/// Returns the talk_proc count if the bytes from `offset` to EOF are exactly
/// one talk block.
fn talk_block_at(data: &[u8], offset: usize) -> Option<u32> {
    let n = data.len();
    let count = u32_at(data, offset)?;
    if count > 1_000_000 {
        return None;
    }
    let mut p = offset + 4;
    for _ in 0..count {
        let windows = i32_at(data, p + TALK_PROC_HEADER - 4)?;
        if !(0..=100_000).contains(&windows) {
            return None;
        }
        p += TALK_PROC_HEADER;
        for _ in 0..windows {
            let text_len = i32_at(data, p + 8)?;
            if text_len < 0 {
                return None;
            }
            p = p.checked_add(12 + text_len as usize * 2)?;
            let options = i32_at(data, p)?;
            if options < 0 {
                return None;
            }
            p = p.checked_add(4 + options as usize * TALK_OPTION_SIZE)?;
            if p > n {
                return None;
            }
        }
    }
    (p == n).then_some(count)
}

fn decode_exporter(raw: &[u8]) -> String {
    let plain: Vec<u8> = raw
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ EXPORTER_KEY[i % 4])
        .take_while(|&b| b != 0)
        .collect();
    GBK.decode(&plain).0.into_owned()
}

/// Incremental builder shared by both parse strategies.
struct Builder {
    data: Vec<u8>,
    p: usize,
    lists: Vec<ListBlock>,
    segments: Vec<Segment>,
    talk_count: u32,
    exporter: Option<String>,
}

impl Builder {
    fn new(data: Vec<u8>) -> Result<Self, ParseError> {
        if data.len() < 8 {
            return Err(ParseError { offset: 0, message: "File is too small to be elements.data".into() });
        }
        Ok(Self {
            data,
            p: 8,
            lists: Vec::new(),
            segments: vec![Segment { kind: SegmentKind::Header, offset: 0, size: 8, before: None }],
            talk_count: 0,
            exporter: None,
        })
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError { offset: self.p, message: message.into() })
    }

    fn push_marker(&mut self, kind: SegmentKind, size: usize) {
        let before = Some(self.lists.len());
        self.segments.push(Segment { kind, offset: self.p, size, before });
        self.p += size;
    }

    fn checksum(&mut self) -> Result<(), ParseError> {
        if self.p + 8 > self.data.len() {
            return self.err("Checksum slot runs past end of file");
        }
        self.push_marker(SegmentKind::Checksum, 8);
        Ok(())
    }

    /// An exporter or tag block; `expected` is the tag value it must carry.
    fn tag_block(&mut self, exporter: bool, strict: bool) -> Result<(), ParseError> {
        let tag = u32_at(&self.data, self.p);
        let expected = if exporter { TAG_EXPORTER } else { TAG_HARDWARE };
        if strict && tag != Some(expected) {
            return self.err(format!("Expected a {} block (tag 0x{expected:08x})", if exporter { "exporter" } else { "tag" }));
        }
        let Some(len) = u32_at(&self.data, self.p + 4).map(|l| l as usize).filter(|&l| l <= MAX_TAG_LEN) else {
            return self.err("Tag block has an invalid length");
        };
        let size = 8 + len + if exporter { 4 } else { 0 };
        if self.p + size > self.data.len() {
            return self.err("Tag block runs past end of file");
        }
        if exporter {
            self.exporter = Some(decode_exporter(&self.data[self.p + 8..self.p + 8 + len]));
        }
        self.push_marker(if exporter { SegmentKind::Exporter } else { SegmentKind::Tag }, size);
        Ok(())
    }

    /// Consumes the talk block if it runs exactly to the end of the file.
    fn talk(&mut self) -> bool {
        let Some(count) = talk_block_at(&self.data, self.p) else { return false };
        let size = self.data.len() - self.p;
        self.segments.push(Segment { kind: SegmentKind::Talk, offset: self.p, size, before: None });
        self.talk_count = count;
        self.p = self.data.len();
        true
    }

    fn list(&mut self) -> Result<(), ParseError> {
        let n = self.data.len() as u64;
        let (Some(item_size), Some(count)) = (u32_at(&self.data, self.p), u32_at(&self.data, self.p + 4)) else {
            return self.err("Unexpected end of file");
        };
        let (item_size, count) = (item_size as u64, count as u64);
        let end = self.p as u64 + 8 + item_size * count;
        if item_size == 0 || item_size > MAX_ITEM_SIZE || end > n {
            return self.err(format!("Expected list {} here, but the bytes are not a list header", self.lists.len()));
        }
        self.lists.push(ListBlock {
            header_offset: self.p,
            data_offset: self.p + 8,
            item_size: item_size as usize,
            count: count as usize,
        });
        self.segments.push(Segment { kind: SegmentKind::List, offset: self.p, size: end as usize - self.p, before: None });
        self.p = end as usize;
        Ok(())
    }

    fn finish(self) -> ElementsFile {
        ElementsFile {
            raw_version: u32_at(&self.data, 0).unwrap(),
            timestamp: u32_at(&self.data, 4).unwrap(),
            data: self.data,
            lists: self.lists,
            segments: self.segments,
            talk_count: self.talk_count,
            exporter: self.exporter,
        }
    }
}

impl ElementsFile {
    /// Parses the file against a marker table. Fails unless every marker is
    /// found at its slot and the talk block ends exactly at EOF.
    pub fn parse_with(data: Vec<u8>, markers: &[Marker]) -> Result<Self, ParseError> {
        let mut sorted = markers.to_vec();
        sorted.sort_by_key(|m| m.before);
        let mut b = Builder::new(data)?;
        let mut next = 0;
        loop {
            while next < sorted.len() && sorted[next].before == b.lists.len() {
                match sorted[next].kind {
                    MarkerKind::Checksum => b.checksum()?,
                    MarkerKind::Exporter => b.tag_block(true, true)?,
                    MarkerKind::Tag => b.tag_block(false, true)?,
                }
                next += 1;
            }
            if b.p >= b.data.len() {
                return b.err("File ended before the NPC dialog block");
            }
            if next == sorted.len() && b.talk() {
                return Ok(b.finish());
            }
            b.list()?;
        }
    }

    /// Parses the file by recognising each segment from its content.
    pub fn parse_detect(data: Vec<u8>) -> Result<Self, ParseError> {
        let mut b = Builder::new(data)?;
        while b.p < b.data.len() {
            match u32_at(&b.data, b.p) {
                Some(TAG_EXPORTER) => b.tag_block(true, false)?,
                Some(TAG_HARDWARE) => b.tag_block(false, false)?,
                _ if is_hex_checksum(&b.data, b.p) => b.checksum()?,
                _ if b.talk() => break,
                _ => b.list()?,
            }
        }
        if b.talk_count == 0 && !b.segments.iter().any(|s| s.kind == SegmentKind::Talk) {
            return b.err("No NPC dialog block found at the end of the file");
        }
        Ok(b.finish())
    }

    /// Format version as used by the game (`ELEMENTDATA_VERSION & 0xffff`).
    pub fn version(&self) -> u32 {
        self.raw_version & 0xffff
    }

    /// Moves everything after list `list` by `delta` bytes and sets its count
    /// (in the struct and in the list header).
    fn resize_list(&mut self, list: usize, count: usize, delta: isize) {
        let start = self.lists[list].header_offset;
        let shift = |o: &mut usize| *o = o.checked_add_signed(delta).expect("offsets stay in the file");
        for block in self.lists.iter_mut().filter(|b| b.header_offset > start) {
            shift(&mut block.header_offset);
            shift(&mut block.data_offset);
        }
        for seg in &mut self.segments {
            if seg.offset > start {
                shift(&mut seg.offset);
            } else if seg.offset == start && seg.kind == SegmentKind::List {
                seg.size = seg.size.checked_add_signed(delta).expect("list size stays positive");
            }
        }
        self.lists[list].count = count;
        self.data[start + 4..start + 8].copy_from_slice(&(count as u32).to_le_bytes());
    }

    /// Inserts a record (of the list's record size) at `row`.
    pub fn insert_record(&mut self, list: usize, row: usize, bytes: &[u8]) {
        let block = &self.lists[list];
        assert!(row <= block.count && bytes.len() == block.item_size, "insert within the list, a whole record");
        let at = block.data_offset + row * block.item_size;
        let count = block.count + 1;
        self.data.splice(at..at, bytes.iter().copied());
        self.resize_list(list, count, bytes.len() as isize);
    }

    /// Removes the record at `row`, returning its bytes.
    pub fn remove_record(&mut self, list: usize, row: usize) -> Vec<u8> {
        let block = &self.lists[list];
        assert!(row < block.count, "remove an existing record");
        let (at, size, count) = (block.data_offset + row * block.item_size, block.item_size, block.count - 1);
        let bytes: Vec<u8> = self.data.drain(at..at + size).collect();
        self.resize_list(list, count, -(size as isize));
        bytes
    }

    pub fn record_offset(&self, list: usize, index: usize) -> Option<usize> {
        let block = self.lists.get(list)?;
        (index < block.count).then(|| block.data_offset + index * block.item_size)
    }

    pub fn record(&self, list: usize, index: usize) -> Option<&[u8]> {
        let offset = self.record_offset(list, index)?;
        let size = self.lists[list].item_size;
        self.data.get(offset..offset + size)
    }

    pub fn item_sizes(&self) -> Vec<usize> {
        self.lists.iter().map(|l| l.item_size).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(before: usize, kind: MarkerKind) -> Marker {
        Marker { before, kind }
    }

    /// A tiny synthetic file: lists of sizes 4 and 8, a checksum slot that
    /// looks like an empty list, one more list, then an empty talk block.
    fn synthetic() -> Vec<u8> {
        let mut d = vec![0x9c, 0, 0, 0x10, 0, 0, 0, 0];
        let list = |d: &mut Vec<u8>, size: u32, count: u32| {
            d.extend(size.to_le_bytes());
            d.extend(count.to_le_bytes());
            d.extend(std::iter::repeat_n(7u8, (size * count) as usize));
        };
        list(&mut d, 4, 2);
        list(&mut d, 8, 1);
        d.extend([0xbc, 0x05, 0, 0, 0, 0, 0, 0]); // raw checksum slot
        list(&mut d, 12, 1);
        d.extend(0u32.to_le_bytes()); // talk block with no entries
        d
    }

    #[test]
    fn marker_table_resolves_raw_checksum_slots() {
        let file = ElementsFile::parse_with(synthetic(), &[marker(2, MarkerKind::Checksum)]).unwrap();
        assert_eq!(file.item_sizes(), vec![4, 8, 12]);
    }

    #[test]
    fn detection_mistakes_raw_checksum_for_empty_list() {
        let file = ElementsFile::parse_detect(synthetic()).unwrap();
        assert_eq!(file.item_sizes(), vec![4, 8, 1468, 12]);
    }

    #[test]
    fn wrong_marker_table_is_rejected() {
        assert!(ElementsFile::parse_with(synthetic(), &[marker(1, MarkerKind::Exporter)]).is_err());
        assert!(ElementsFile::parse_with(synthetic(), &[marker(9, MarkerKind::Checksum)]).is_err());
    }

    #[test]
    fn rejects_garbage() {
        let mut data = vec![0x9c, 0, 0, 0x10, 0, 0, 0, 0];
        data.extend_from_slice(&[0xff; 16]);
        assert!(ElementsFile::parse_detect(data.clone()).is_err());
        assert!(ElementsFile::parse_with(data, &[]).is_err());
    }
}
