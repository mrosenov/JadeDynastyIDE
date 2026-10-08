//! The outer `tasks.data` container.
//!
//! `tasks.data` is an index. Its numbered companions (`tasks.data1`,
//! `tasks.data2`, ...) contain up to 300 top-level task trees apiece.
//! The index stores one MD5 digest per companion, while every companion has a
//! header followed by absolute offsets to its root records.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

pub const INDEX_MAGIC: u32 = 0x6934_0304;
pub const PACK_MAGIC: u32 = 0x0693_4554;
pub const ROOTS_PER_PACK: usize = 300;

const INDEX_HEADER_SIZE: usize = 20;
const DIGEST_SIZE: usize = 16;
const PACK_HEADER_SIZE: usize = 8;
const MAX_PACKS: usize = 10_000;
const MAX_ROOTS: usize = ROOTS_PER_PACK * MAX_PACKS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexHeader {
    pub version: u32,
    pub export_version: u32,
    pub root_count: u32,
    pub pack_count: u32,
}

#[derive(Debug, Clone)]
pub struct Pack {
    path: PathBuf,
    pub number: usize,
    pub size: u64,
    pub digest: [u8; DIGEST_SIZE],
    /// Absolute file offsets, one per top-level task tree.
    pub root_offsets: Vec<u32>,
}

impl Pack {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root_count(&self) -> usize {
        self.root_offsets.len()
    }

    pub fn root_range(&self, root: usize) -> Result<std::ops::Range<u64>, String> {
        let start = self.root_offsets.get(root).copied().ok_or_else(|| {
            format!(
                "tasks.data{}: root {} does not exist",
                self.number,
                root + 1
            )
        })? as u64;
        let end = self
            .root_offsets
            .get(root + 1)
            .copied()
            .map(u64::from)
            .unwrap_or(self.size);
        if start >= end || end > self.size {
            return Err(format!(
                "tasks.data{}: root {} has invalid range {start}..{end}",
                self.number,
                root + 1
            ));
        }
        Ok(start..end)
    }

    /// Reads one complete top-level task tree without loading the rest of the
    /// pack. Child tasks are part of the returned record.
    pub fn read_root(&self, root: usize) -> Result<Vec<u8>, String> {
        let range = self.root_range(root)?;
        let len: usize = (range.end - range.start)
            .try_into()
            .map_err(|_| format!("tasks.data{}: root {} is too large", self.number, root + 1))?;
        let mut file =
            File::open(&self.path).map_err(|error| format!("{}: {error}", self.path.display()))?;
        file.seek(SeekFrom::Start(range.start))
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        let mut bytes = vec![0; len];
        file.read_exact(&mut bytes)
            .map_err(|error| format!("{}: {error}", self.path.display()))?;
        Ok(bytes)
    }
}

#[derive(Debug, Clone)]
pub struct TaskContainer {
    index_path: PathBuf,
    pub header: IndexHeader,
    pub packs: Vec<Pack>,
}

impl TaskContainer {
    /// Opens and verifies the complete static task set. Pack contents remain
    /// on disk; only their headers and offset tables are kept in memory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let index_path = path.as_ref().to_path_buf();
        let index = std::fs::read(&index_path)
            .map_err(|error| format!("{}: {error}", index_path.display()))?;
        let header = parse_index_header(&index, &index_path)?;
        let pack_count = header.pack_count as usize;
        let expected_size = INDEX_HEADER_SIZE
            .checked_add(
                pack_count
                    .checked_mul(DIGEST_SIZE)
                    .ok_or("tasks.data: pack count is too large")?,
            )
            .ok_or("tasks.data: index size overflow")?;
        if index.len() != expected_size {
            return Err(format!(
                "{}: index is {} bytes, expected {expected_size} for {pack_count} packs",
                index_path.display(),
                index.len()
            ));
        }

        let mut packs = Vec::with_capacity(pack_count);
        let mut found_roots = 0usize;
        for pack_index in 0..pack_count {
            let number = pack_index + 1;
            let digest_at = INDEX_HEADER_SIZE + pack_index * DIGEST_SIZE;
            let digest: [u8; DIGEST_SIZE] = index[digest_at..digest_at + DIGEST_SIZE]
                .try_into()
                .unwrap();
            let pack = open_pack(&index_path, number, digest)?;
            found_roots = found_roots
                .checked_add(pack.root_count())
                .ok_or_else(|| format!("{}: root count overflow", index_path.display()))?;
            packs.push(pack);
        }
        if found_roots != header.root_count as usize {
            return Err(format!(
                "{}: index declares {} roots but its packs contain {found_roots}",
                index_path.display(),
                header.root_count
            ));
        }

        Ok(Self {
            index_path,
            header,
            packs,
        })
    }

    pub fn index_path(&self) -> &Path {
        &self.index_path
    }

    pub fn total_pack_bytes(&self) -> u64 {
        self.packs.iter().map(|pack| pack.size).sum()
    }

    pub fn root(&self, pack: usize, root: usize) -> Result<Vec<u8>, String> {
        self.packs
            .get(pack)
            .ok_or_else(|| {
                format!(
                    "{}: pack {} does not exist",
                    self.index_path.display(),
                    pack + 1
                )
            })?
            .read_root(root)
    }
}

fn parse_index_header(index: &[u8], path: &Path) -> Result<IndexHeader, String> {
    if index.len() < INDEX_HEADER_SIZE {
        return Err(format!("{}: index header is truncated", path.display()));
    }
    let magic = u32_at(index, 0).unwrap();
    if magic != INDEX_MAGIC {
        return Err(format!(
            "{}: bad task index magic 0x{magic:08X}",
            path.display()
        ));
    }
    let header = IndexHeader {
        version: u32_at(index, 4).unwrap(),
        export_version: u32_at(index, 8).unwrap(),
        root_count: u32_at(index, 12).unwrap(),
        pack_count: u32_at(index, 16).unwrap(),
    };
    let packs = header.pack_count as usize;
    let roots = header.root_count as usize;
    if packs == 0 || packs > MAX_PACKS {
        return Err(format!(
            "{}: unreasonable pack count {}",
            path.display(),
            header.pack_count
        ));
    }
    if roots == 0 || roots > MAX_ROOTS {
        return Err(format!(
            "{}: unreasonable root count {}",
            path.display(),
            header.root_count
        ));
    }
    if roots > packs * ROOTS_PER_PACK || roots <= (packs - 1) * ROOTS_PER_PACK {
        return Err(format!(
            "{}: {} roots cannot be stored in {} packs of at most {ROOTS_PER_PACK}",
            path.display(),
            header.root_count,
            header.pack_count
        ));
    }
    Ok(header)
}

fn open_pack(
    index_path: &Path,
    number: usize,
    expected_digest: [u8; DIGEST_SIZE],
) -> Result<Pack, String> {
    let path = numbered_pack_path(index_path, number);
    let size = std::fs::metadata(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size < PACK_HEADER_SIZE as u64 {
        return Err(format!("{}: pack header is truncated", path.display()));
    }
    let actual_digest = file_md5(&path)?;
    if actual_digest != expected_digest {
        return Err(format!(
            "{}: MD5 mismatch (index {}, actual {})",
            path.display(),
            hex(&expected_digest),
            hex(&actual_digest)
        ));
    }

    let file = File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut header = [0u8; PACK_HEADER_SIZE];
    reader
        .read_exact(&mut header)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let magic = u32_at(&header, 0).unwrap();
    if magic != PACK_MAGIC {
        return Err(format!(
            "{}: bad task pack magic 0x{magic:08X}",
            path.display()
        ));
    }
    let root_count = u32_at(&header, 4).unwrap() as usize;
    if root_count == 0 || root_count > ROOTS_PER_PACK {
        return Err(format!(
            "{}: unreasonable root count {root_count}",
            path.display()
        ));
    }
    let table_bytes = root_count
        .checked_mul(4)
        .ok_or_else(|| format!("{}: offset table overflow", path.display()))?;
    let data_start = PACK_HEADER_SIZE + table_bytes;
    if data_start as u64 >= size {
        return Err(format!("{}: pack has no root data", path.display()));
    }
    let mut table = vec![0u8; table_bytes];
    reader
        .read_exact(&mut table)
        .map_err(|error| format!("{}: truncated root offset table: {error}", path.display()))?;
    let root_offsets: Vec<u32> = table
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    if root_offsets.first().copied() != Some(data_start as u32) {
        return Err(format!(
            "{}: first root starts at {}, expected {data_start}",
            path.display(),
            root_offsets.first().copied().unwrap_or(0)
        ));
    }
    for (index, &offset) in root_offsets.iter().enumerate() {
        if offset as u64 >= size {
            return Err(format!(
                "{}: root {} offset {offset} is outside the {size}-byte pack",
                path.display(),
                index + 1
            ));
        }
        if index > 0 && root_offsets[index - 1] >= offset {
            return Err(format!(
                "{}: root offsets are not strictly increasing at root {} ({} then {offset})",
                path.display(),
                index + 1,
                root_offsets[index - 1]
            ));
        }
    }

    Ok(Pack {
        path,
        number,
        size,
        digest: actual_digest,
        root_offsets,
    })
}

fn numbered_pack_path(index: &Path, number: usize) -> PathBuf {
    let mut name: OsString = index.as_os_str().to_owned();
    name.push(number.to_string());
    PathBuf::from(name)
}

fn file_md5(path: &Path) -> Result<[u8; DIGEST_SIZE], String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut reader = BufReader::with_capacity(128 * 1024, file);
    let mut digest = Md5::new();
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().into())
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jdide-tasks-{name}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture(name: &str) -> (PathBuf, Vec<Vec<u8>>) {
        let dir = temp_dir(name);
        let index = dir.join("tasks.data");
        let roots = vec![
            b"first task tree".to_vec(),
            b"second task tree with child".to_vec(),
        ];
        let data_start = (PACK_HEADER_SIZE + roots.len() * 4) as u32;
        let mut pack = Vec::new();
        pack.extend_from_slice(&PACK_MAGIC.to_le_bytes());
        pack.extend_from_slice(&(roots.len() as u32).to_le_bytes());
        let mut offset = data_start;
        for root in &roots {
            pack.extend_from_slice(&offset.to_le_bytes());
            offset += root.len() as u32;
        }
        for root in &roots {
            pack.extend_from_slice(root);
        }
        std::fs::write(numbered_pack_path(&index, 1), &pack).unwrap();
        let digest: [u8; DIGEST_SIZE] = Md5::digest(&pack).into();
        let mut contents = Vec::new();
        contents.extend_from_slice(&INDEX_MAGIC.to_le_bytes());
        contents.extend_from_slice(&165u32.to_le_bytes());
        contents.extend_from_slice(&1u32.to_le_bytes());
        contents.extend_from_slice(&(roots.len() as u32).to_le_bytes());
        contents.extend_from_slice(&1u32.to_le_bytes());
        contents.extend_from_slice(&digest);
        std::fs::write(&index, contents).unwrap();
        (index, roots)
    }

    #[test]
    fn opens_index_and_reads_roots_lazily() {
        let (index, roots) = fixture("open");
        let tasks = TaskContainer::open(&index).unwrap();
        assert_eq!(tasks.header.version, 165);
        assert_eq!(tasks.header.export_version, 1);
        assert_eq!(tasks.header.root_count, 2);
        assert_eq!(tasks.packs.len(), 1);
        assert_eq!(tasks.packs[0].root_count(), 2);
        assert_eq!(tasks.root(0, 0).unwrap(), roots[0]);
        assert_eq!(tasks.root(0, 1).unwrap(), roots[1]);
        std::fs::remove_dir_all(index.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_pack_changed_after_index_was_written() {
        let (index, _) = fixture("digest");
        let pack = numbered_pack_path(&index, 1);
        let mut bytes = std::fs::read(&pack).unwrap();
        *bytes.last_mut().unwrap() ^= 0xff;
        std::fs::write(&pack, bytes).unwrap();
        let error = TaskContainer::open(&index).unwrap_err();
        assert!(error.contains("MD5 mismatch"), "{error}");
        std::fs::remove_dir_all(index.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_invalid_root_offsets_even_with_a_matching_digest() {
        let (index, _) = fixture("offsets");
        let pack = numbered_pack_path(&index, 1);
        let mut bytes = std::fs::read(&pack).unwrap();
        bytes[12..16].copy_from_slice(&12u32.to_le_bytes());
        std::fs::write(&pack, &bytes).unwrap();
        let digest: [u8; DIGEST_SIZE] = Md5::digest(&bytes).into();
        let mut index_bytes = std::fs::read(&index).unwrap();
        index_bytes[INDEX_HEADER_SIZE..INDEX_HEADER_SIZE + DIGEST_SIZE].copy_from_slice(&digest);
        std::fs::write(&index, index_bytes).unwrap();
        let error = TaskContainer::open(&index).unwrap_err();
        assert!(error.contains("not strictly increasing"), "{error}");
        std::fs::remove_dir_all(index.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_missing_pack() {
        let (index, _) = fixture("missing");
        std::fs::remove_file(numbered_pack_path(&index, 1)).unwrap();
        let error = TaskContainer::open(&index).unwrap_err();
        assert!(error.contains("tasks.data1"), "{error}");
        std::fs::remove_dir_all(index.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_truncated_index() {
        let (index, _) = fixture("truncated");
        let mut bytes = std::fs::read(&index).unwrap();
        bytes.truncate(INDEX_HEADER_SIZE - 1);
        std::fs::write(&index, bytes).unwrap();
        let error = TaskContainer::open(&index).unwrap_err();
        assert!(error.contains("index header is truncated"), "{error}");
        std::fs::remove_dir_all(index.parent().unwrap()).unwrap();
    }

    #[test]
    fn opens_real_task_sets_when_available() {
        let root = PathBuf::from(std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/".into()));
        let samples = [
            (
                root.join("Games/XtremeJade/element/data/tasks.data"),
                165,
                13_450,
                45,
            ),
            (
                root.join("Games/ForsakenJD/element/data/tasks.data"),
                172,
                15_211,
                51,
            ),
            (
                root.join("Games/Elite Jade Dynasty - HDN/element/data/tasks.data"),
                184,
                17_882,
                60,
            ),
            (
                root.join("Game Dev/JD/1792/gamed/config/tasks.data"),
                184,
                17_860,
                60,
            ),
        ];
        for (path, version, roots, packs) in samples {
            if !path.is_file() {
                continue;
            }
            let tasks = TaskContainer::open(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(tasks.header.version, version, "{}", path.display());
            // The user's client sets gain top-level tasks when clones are saved, so the
            // original count is a minimum.
            assert!(tasks.header.root_count >= roots, "{}: {} roots", path.display(), tasks.header.root_count);
            assert_eq!(tasks.packs.len(), packs, "{}", path.display());
            assert!(!tasks.root(0, 0).unwrap().is_empty(), "{}", path.display());
            assert!(
                !tasks
                    .root(packs - 1, tasks.packs[packs - 1].root_count() - 1)
                    .unwrap()
                    .is_empty(),
                "{}",
                path.display()
            );
        }
    }
}
