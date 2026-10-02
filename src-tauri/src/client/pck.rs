//! Reader for Angelica File Packages (`.pck`, version 2.2), the archives the
//! client keeps its resources in.
//!
//! Layout (from AFilePackage.h and the files themselves):
//!
//! ```text
//! u32 0x4DCA23EF, u32 total size, u32 0x56A089B7     "safe header"
//! file data …                                         each zlib-compressed or stored
//! entry table at (header.entry_offset ^ KEY1):
//!     per file: u32 len ^ KEY1, u32 len ^ KEY2, len bytes of FILEENTRY
//!               (zlib-compressed when len < 276)
//! header (272 B): u32 0xFDFDFEEE, u32 version, u32 entry_offset ^ KEY1,
//!                 u32 flags, char description[252], u32 0xF00DBEEF
//! u32 file count, u32 version (0x00020002)
//! ```
//!
//! Archives over 2 GB continue in `.pkx` (then `.pkx1`, …) files; offsets
//! count across all parts.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use encoding_rs::GBK;
use flate2::read::ZlibDecoder;

const KEY1: u32 = 0xa893_7462;
const KEY2: u32 = 0xf1a4_3653;
const HEADER_GUARD0: u32 = 0xfdfd_feee;
const HEADER_GUARD1: u32 = 0xf00d_beef;
const HEADER_SIZE: u64 = 272;
const ENTRY_SIZE: usize = 276;
const NAME_SIZE: usize = 260;
const MAX_FILE: u32 = 512 << 20;

#[derive(Debug, Clone)]
pub struct PckEntry {
    /// Path inside the package as stored (backslashes, original case).
    pub path: String,
    pub offset: u32,
    pub length: u32,
    pub compressed: u32,
}

struct Part {
    file: File,
    start: u64,
    len: u64,
}

pub struct Pck {
    pub path: PathBuf,
    parts: Mutex<Vec<Part>>,
    pub entries: Vec<PckEntry>,
    /// Normalized path (lowercase, backslashes) → entry index.
    index: HashMap<String, usize>,
}

/// Lowercase, backslash-separated path for lookups.
pub fn normalize(path: &str) -> String {
    path.replace('/', "\\").trim_start_matches('\\').to_lowercase()
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn err(path: &Path, what: impl std::fmt::Display) -> String {
    format!("{}: {what}", path.display())
}

impl Pck {
    pub fn open(path: &Path) -> Result<Self, String> {
        // The .pck and any continuation parts, read as one stream.
        let mut files = vec![path.to_path_buf()];
        for n in 0.. {
            let ext = if n == 0 { "pkx".to_string() } else { format!("pkx{n}") };
            let part = path.with_extension(ext);
            if !part.exists() {
                break;
            }
            files.push(part);
        }
        let mut parts = Vec::new();
        let mut start = 0;
        for f in &files {
            let file = File::open(f).map_err(|e| err(f, e))?;
            let len = file.metadata().map_err(|e| err(f, e))?.len();
            parts.push(Part { file, start, len });
            start += len;
        }
        let total = start;
        if total < HEADER_SIZE + 8 + 12 {
            return Err(err(path, "too small to be a package"));
        }

        let mut pck = Self { path: path.to_path_buf(), parts: Mutex::new(parts), entries: Vec::new(), index: HashMap::new() };
        let tail = pck.read_at(total - 8, 8)?;
        let (count, version) = (u32_at(&tail, 0), u32_at(&tail, 4));
        if version != 0x0002_0002 {
            return Err(err(path, format!("unsupported package version 0x{version:08x}")));
        }
        let header = pck.read_at(total - 8 - HEADER_SIZE, HEADER_SIZE as usize)?;
        if u32_at(&header, 0) != HEADER_GUARD0 || u32_at(&header, 268) != HEADER_GUARD1 {
            return Err(err(path, "package header not found"));
        }
        let entry_offset = (u32_at(&header, 8) ^ KEY1) as u64;
        let table_len = (total - 8 - HEADER_SIZE).checked_sub(entry_offset).ok_or_else(|| err(path, "bad entry table offset"))?;
        let table = pck.read_at(entry_offset, table_len as usize)?;

        let mut p = 0;
        let mut entries = Vec::with_capacity(count as usize);
        for i in 0..count {
            let bad = || err(path, format!("entry {i} is damaged"));
            if p + 8 > table.len() {
                return Err(bad());
            }
            let len = (u32_at(&table, p) ^ KEY1) as usize;
            if u32_at(&table, p + 4) ^ KEY2 != len as u32 || p + 8 + len > table.len() {
                return Err(bad());
            }
            let raw = &table[p + 8..p + 8 + len];
            p += 8 + len;
            let entry = if len < ENTRY_SIZE {
                let mut out = Vec::with_capacity(ENTRY_SIZE);
                ZlibDecoder::new(raw).read_to_end(&mut out).map_err(|_| bad())?;
                out
            } else {
                raw.to_vec()
            };
            if entry.len() < NAME_SIZE + 12 {
                return Err(bad());
            }
            let name_end = entry[..NAME_SIZE].iter().position(|&b| b == 0).unwrap_or(NAME_SIZE);
            entries.push(PckEntry {
                path: GBK.decode(&entry[..name_end]).0.into_owned(),
                offset: u32_at(&entry, NAME_SIZE),
                length: u32_at(&entry, NAME_SIZE + 4),
                compressed: u32_at(&entry, NAME_SIZE + 8),
            });
        }
        pck.index = entries.iter().enumerate().map(|(i, e)| (normalize(&e.path), i)).collect();
        pck.entries = entries;
        Ok(pck)
    }

    fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut parts = self.parts.lock().map_err(|_| "package lock poisoned")?;
        let mut out = vec![0u8; len];
        let mut done = 0;
        while done < len {
            let at = offset + done as u64;
            let part = parts
                .iter_mut()
                .find(|p| at >= p.start && at < p.start + p.len)
                .ok_or_else(|| err(&self.path, format!("read past the end at {at}")))?;
            let n = ((part.start + part.len - at) as usize).min(len - done);
            part.file.seek(SeekFrom::Start(at - part.start)).map_err(|e| err(&self.path, e))?;
            part.file.read_exact(&mut out[done..done + n]).map_err(|e| err(&self.path, e))?;
            done += n;
        }
        Ok(out)
    }

    pub fn find(&self, path: &str) -> Option<&PckEntry> {
        self.index.get(&normalize(path)).map(|&i| &self.entries[i])
    }

    /// The contents of a file in the package.
    pub fn read(&self, entry: &PckEntry) -> Result<Vec<u8>, String> {
        if entry.length > MAX_FILE {
            return Err(err(&self.path, format!("{} is too large", entry.path)));
        }
        let stored = self.read_at(entry.offset as u64, entry.compressed as usize)?;
        if entry.compressed >= entry.length {
            return Ok(stored);
        }
        let mut out = Vec::with_capacity(entry.length as usize);
        ZlibDecoder::new(&stored[..])
            .read_to_end(&mut out)
            .map_err(|e| err(&self.path, format!("{}: {e}", entry.path)))?;
        Ok(out)
    }

    pub fn read_path(&self, path: &str) -> Result<Vec<u8>, String> {
        let entry = self.find(path).ok_or_else(|| err(&self.path, format!("{path} is not in the package")))?;
        self.read(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var("JDIDE_CLIENT").unwrap_or_else(|_| "E:/Games/ForsakenJD".into()));
        dir.join("element/surfaces.pck").exists().then_some(dir)
    }

    #[test]
    fn reads_the_surfaces_package() {
        let Some(dir) = client() else { return eprintln!("skipping: no client found") };
        let pck = Pck::open(&dir.join("element/surfaces.pck")).unwrap();
        assert!(pck.entries.len() > 10_000);
        let txt = pck.read_path("Surfaces/IconSet/iconlist_ivtr.txt").unwrap();
        let text = GBK.decode(&txt).0;
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("36"));
        let dds = pck.read_path("surfaces\\iconset\\iconlist_ivtr.dds").unwrap();
        assert_eq!(&dds[..4], b"DDS ");
    }

    #[test]
    fn reads_packages_split_into_pkx_parts() {
        let Some(dir) = client() else { return };
        let models = dir.join("element/models.pck");
        if !models.with_extension("pkx").exists() {
            return;
        }
        let pck = Pck::open(&models).unwrap();
        // A file stored past the 2 GB .pck part must come from the .pkx.
        let far = pck.entries.iter().max_by_key(|e| e.offset).unwrap();
        assert!(far.offset as u64 > std::fs::metadata(&models).unwrap().len());
        assert_eq!(pck.read(far).unwrap().len(), far.length as usize);
    }
}
