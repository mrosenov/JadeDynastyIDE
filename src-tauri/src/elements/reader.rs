//! Structural reader for `elements.data`.
//!
//! Layout, as written by `elementdataman::save_data`:
//!
//! ```text
//! u32 version (low 16 bits = format version, high bits = 0x1000)
//! u32 export timestamp
//! repeated:
//!     list      u32 item_size, u32 count, count * item_size bytes
//!     checksum  8 ASCII hex chars (a slice of the MD5 written by the exporter)
//!     tag       u32 0x19e75edf, u32 len, len bytes (exporter name), u32 time
//!     tag       u32 0xee35679f, u32 len, len bytes (hardware info)
//! talk block    u32 count, count * talk_proc (variable length), up to EOF
//! ```
//!
//! The reader identifies each segment from its content rather than from a
//! per-version table, so newer versions with extra lists parse unchanged.

use encoding_rs::GBK;
use serde::Serialize;

pub const TAG_EXPORTER: u32 = 0x19e7_5edf;
pub const TAG_HARDWARE: u32 = 0xee35_679f;

const EXPORTER_KEY: [u8; 4] = [0x5f, 0x6d, 0xe8, 0xc9];
const MAX_ITEM_SIZE: u64 = 1 << 20;
const TALK_PROC_HEADER: usize = 4 + 128 + 4; // id_talk, text[64], num_window
const TALK_OPTION_SIZE: usize = 4 + 128 + 4; // id, text[64], param

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SegmentKind {
    Header,
    List,
    Checksum,
    Tag,
    Talk,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub offset: usize,
    pub size: usize,
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

fn is_checksum(data: &[u8], offset: usize) -> bool {
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

impl ElementsFile {
    pub fn parse(data: Vec<u8>) -> Result<Self, ParseError> {
        let n = data.len();
        let err = |offset: usize, message: &str| ParseError { offset, message: message.to_string() };
        if n < 8 {
            return Err(err(0, "File is too small to be elements.data"));
        }
        let raw_version = u32_at(&data, 0).unwrap();
        let timestamp = u32_at(&data, 4).unwrap();

        let mut segments = vec![Segment { kind: SegmentKind::Header, offset: 0, size: 8 }];
        let mut lists = Vec::new();
        let mut talk_count = 0;
        let mut exporter = None;
        let mut p = 8;

        while p < n {
            let word = u32_at(&data, p).ok_or_else(|| err(p, "Unexpected end of file"))?;

            if word == TAG_EXPORTER || word == TAG_HARDWARE {
                let len = u32_at(&data, p + 4).ok_or_else(|| err(p, "Truncated tag block"))? as usize;
                let trailer = if word == TAG_EXPORTER { 4 } else { 0 };
                let size = 8 + len + trailer;
                if p + size > n {
                    return Err(err(p, "Tag block runs past end of file"));
                }
                if word == TAG_EXPORTER {
                    exporter = Some(decode_exporter(&data[p + 8..p + 8 + len]));
                }
                segments.push(Segment { kind: SegmentKind::Tag, offset: p, size });
                p += size;
                continue;
            }

            if is_checksum(&data, p) {
                segments.push(Segment { kind: SegmentKind::Checksum, offset: p, size: 8 });
                p += 8;
                continue;
            }

            if let Some(count) = talk_block_at(&data, p) {
                segments.push(Segment { kind: SegmentKind::Talk, offset: p, size: n - p });
                talk_count = count;
                break;
            }

            let item_size = word as u64;
            let count = u32_at(&data, p + 4).ok_or_else(|| err(p, "Truncated list header"))? as u64;
            let end = p as u64 + 8 + item_size * count;
            if item_size == 0 || item_size > MAX_ITEM_SIZE || end > n as u64 {
                return Err(err(
                    p,
                    &format!("Unrecognised data after list {} (not a list, checksum, tag or talk block)", lists.len()),
                ));
            }
            lists.push(ListBlock {
                header_offset: p,
                data_offset: p + 8,
                item_size: item_size as usize,
                count: count as usize,
            });
            segments.push(Segment { kind: SegmentKind::List, offset: p, size: (end as usize) - p });
            p = end as usize;
        }

        Ok(Self { data, raw_version, timestamp, lists, segments, talk_count, exporter })
    }

    /// Format version as used by the game (`ELEMENTDATA_VERSION & 0xffff`).
    pub fn version(&self) -> u32 {
        self.raw_version & 0xffff
    }

    /// Marker positions such as `c13 t23`: a checksum or tag after N lists.
    /// Files with the same signature share the same list grouping.
    pub fn layout_signature(&self) -> String {
        let mut lists_seen = 0;
        let mut parts = Vec::new();
        for s in &self.segments {
            match s.kind {
                SegmentKind::List => lists_seen += 1,
                SegmentKind::Checksum => parts.push(format!("c{lists_seen}")),
                SegmentKind::Tag => parts.push(format!("t{lists_seen}")),
                SegmentKind::Header | SegmentKind::Talk => {}
            }
        }
        parts.join(" ")
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Real files are not checked in; point JDIDE_SAMPLES at the JD folder to run these.
    fn sample(rel: &str) -> Option<Vec<u8>> {
        let root = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/Game Dev/JD".into());
        std::fs::read(PathBuf::from(root).join(rel)).ok()
    }

    fn check(rel: &str, version: u32, lists: usize, layout: &str) {
        let Some(data) = sample(rel) else {
            eprintln!("skipping {rel}: sample not found");
            return;
        };
        let len = data.len();
        let file = ElementsFile::parse(data).expect(rel);
        assert_eq!(file.version(), version, "{rel}");
        assert_eq!(file.lists.len(), lists, "{rel}");
        assert_eq!(file.layout_signature(), layout, "{rel}");
        let covered: usize = file.segments.iter().map(|s| s.size).sum();
        assert_eq!(covered, len, "{rel}: segments must cover the whole file");
        assert!(file.talk_count > 0, "{rel}");
    }

    #[test]
    fn parses_v156_server_file() {
        check("zxserver/zgame/gs/config/elements.data", 156, 193, "c13 t23 c36 t56 c62 c103");
    }

    #[test]
    fn parses_newer_versions() {
        check("1559/gamed/config/elements.data", 158, 230, "c13 t23 c36 t56 c62 c103");
        check("Clean/root/gamed/config/elements.data", 160, 246, "c13 t23 c36 t56 c62 c103");
        check("1792/gamed/config/elements.data", 165, 319, "c13 t23 c36 t56 c62 c103");
    }

    #[test]
    fn parses_older_versions() {
        check("Tools/JadeEditorFOX/tests/elements - Copy.data", 112, 107, "t23 t56");
        check("1792/gamed/config/c01/elements.data", 66, 89, "t22 t54");
    }

    #[test]
    fn rejects_garbage() {
        let mut data = vec![0x9c, 0, 0, 0x10, 0, 0, 0, 0];
        data.extend_from_slice(&[0xff; 16]);
        assert!(ElementsFile::parse(data).is_err());
    }
}
