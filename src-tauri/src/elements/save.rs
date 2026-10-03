//! Saving elements.data the way the official tools write it
//! (`elementdataman::save_data`, used by the editor and the localiser).
//!
//! The data in memory already holds the edits; saving writes it out with:
//!
//! - the export time in the header set to now,
//! - the integrity digest the client checks in `load_data` (it refuses to
//!   start on a mismatch):
//!
//!   ```text
//!   MD5( "ZPWDATA" + path.data + elements.data without its 8-byte digest slots )
//!   ```
//!
//!   as 32 lowercase hex characters, 8 in each of the first four slots. The
//!   path.data must be the one the client ships with this elements.data.
//!
//! Lists, markers, the exporter block and the talk block are written as they
//! are. The file is written next to the target and then moved over it, so a
//! failed save never leaves half a file; the first save over an existing file
//! keeps a timestamped backup of it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::reader::SegmentKind;
use super::Document;

const SALT: &[u8] = b"ZPWDATA";
/// Slots holding the digest; the client reads these four (later slots are
/// skipped when hashing but hold no part of it).
const DIGEST_SLOTS: usize = 4;

/// Size and modification time of a file, to notice changes made by others.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl DiskStamp {
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self { len: meta.len(), modified: meta.modified().ok() })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumStatus {
    /// The file on disk carries the digest of itself with this path.data.
    Valid,
    /// It carries another digest: an edited file, or another path.data.
    Mismatch,
    /// Its slots do not hold a digest (written by a tool that skips it).
    NotStored,
    /// No path.data was found, so no digest can be written.
    NoPathData,
    /// This version has no digest slots.
    NoSlots,
}

/// Which path.data the digest is made with, and how the file on disk fares.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumCheck {
    pub status: ChecksumStatus,
    pub slots: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_data: Option<String>,
    /// Where it was found: "chosen", "next to the file" or "client folder".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_data_from: Option<&'static str>,
    /// The digest stored in the file on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored: Option<String>,
    /// The digest that file should carry with this path.data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// The file the check was made on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<String>,
}

/// What a save would do, for the dialog that asks first.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlan {
    pub path: String,
    /// The target exists and is replaced.
    pub replaces: bool,
    /// The target is the file that is open.
    pub same_file: bool,
    /// The open file was changed on disk by something else since it was read.
    pub changed_on_disk: bool,
    pub read_only: bool,
    pub size: usize,
    pub changed: usize,
    pub added: usize,
    pub deleted: usize,
    pub checksum: ChecksumCheck,
    /// The backup the save would make (when asked for).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOptions {
    pub path: String,
    /// path.data for the digest; found by itself when not given.
    #[serde(default)]
    pub path_data: Option<String>,
    /// Keep a copy of the replaced file (once per file and session).
    #[serde(default)]
    pub backup: bool,
    /// Replace the open file even when another program changed it since
    /// it was read (otherwise the save stops with [`CHANGED_ON_DISK`]).
    #[serde(default)]
    pub replace_changed: bool,
}

/// Error prefix of a save stopped because the file changed on disk.
pub const CHANGED_ON_DISK: &str = "CHANGED_ON_DISK";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    /// The digest written, or none when it could not be made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    pub timestamp: u32,
}

/// Offsets of the digest slots in file order.
fn slots_of(segments: &[super::reader::Segment]) -> Vec<usize> {
    segments.iter().filter(|s| s.kind == SegmentKind::Checksum).map(|s| s.offset).collect()
}

/// The digest the client expects for `data` (its slots at `slots`).
pub fn digest(data: &[u8], slots: &[usize], path_data: &[u8]) -> String {
    let mut md5 = Md5::new();
    md5.update(SALT);
    md5.update(path_data);
    let mut at = 0;
    for &slot in slots {
        md5.update(&data[at..slot]);
        at = slot + 8;
    }
    md5.update(&data[at..]);
    md5.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The digest stored in the first four slots, when they hold hex text.
pub fn stored_digest(data: &[u8], slots: &[usize]) -> Option<String> {
    if slots.len() < DIGEST_SLOTS {
        return None;
    }
    let text: Vec<u8> = slots[..DIGEST_SLOTS].iter().flat_map(|&s| data[s..s + 8].iter().copied()).collect();
    text.iter().all(u8::is_ascii_hexdigit).then(|| String::from_utf8(text).unwrap().to_lowercase())
}

/// `elements.data` → `elements.data.20261003-143205.bak`, next to it.
fn backup_path(target: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "elements.data".into());
    target.with_file_name(format!("{name}.{stamp}.bak"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl Document {
    /// Whether another program wrote the open file since it was read or saved.
    fn changed_on_disk(&self) -> bool {
        self.disk.is_some() && DiskStamp::of(Path::new(&self.path)) != self.disk
    }

    fn checksum_slots(&self) -> Vec<usize> {
        slots_of(&self.file.segments)
    }

    /// The path.data the digest is made with: the chosen one, else the one
    /// next to the target, next to the open file, or in the client folder.
    fn find_path_data(&self, target: &Path, chosen: Option<&str>) -> Option<(PathBuf, &'static str)> {
        if let Some(p) = chosen.map(str::trim).filter(|p| !p.is_empty()) {
            return Some((PathBuf::from(p), "chosen"));
        }
        let beside = |p: &Path| p.parent().map(|d| d.join("path.data")).filter(|p| p.is_file());
        beside(target)
            .or_else(|| beside(Path::new(&self.path)))
            .map(|p| (p, "next to the file"))
            .or_else(|| self.resources.as_ref().map(|r| r.path_data_file()).filter(|p| p.is_file()).map(|p| (p, "client folder")))
    }

    /// Checks the digest of a file on disk (the target, else the open file)
    /// against the path.data that will be used.
    fn check_checksum(&self, target: &Path, chosen: Option<&str>) -> Result<ChecksumCheck, String> {
        let slots = self.checksum_slots();
        let found = self.find_path_data(target, chosen);
        let mut check = ChecksumCheck {
            status: ChecksumStatus::NoSlots,
            slots: slots.len(),
            path_data: found.as_ref().map(|(p, _)| p.display().to_string()),
            path_data_from: found.as_ref().map(|(_, from)| *from),
            stored: None,
            expected: None,
            checked: None,
        };
        if slots.len() < DIGEST_SLOTS {
            return Ok(check);
        }
        let Some((path_data, _)) = found else {
            check.status = ChecksumStatus::NoPathData;
            return Ok(check);
        };
        let path_data = std::fs::read(&path_data).map_err(|e| format!("Could not read {}: {e}", path_data.display()))?;
        // The file on disk, if it still has the open file's structure.
        let on_disk = [target, Path::new(&self.path)]
            .into_iter()
            .find_map(|p| std::fs::read(p).ok().filter(|d| self.same_layout(d)).map(|d| (p, d)));
        let Some((checked, data)) = on_disk else {
            check.status = ChecksumStatus::NotStored;
            return Ok(check);
        };
        let slots = self.slots_in(&data);
        check.checked = Some(checked.display().to_string());
        check.stored = stored_digest(&data, &slots);
        check.expected = Some(digest(&data, &slots, &path_data));
        check.status = match &check.stored {
            None => ChecksumStatus::NotStored,
            Some(s) if Some(s) == check.expected.as_ref() => ChecksumStatus::Valid,
            Some(_) => ChecksumStatus::Mismatch,
        };
        Ok(check)
    }

    /// Whether another copy of the file has its digest slots where they
    /// would be in the file as opened (the same version and lists).
    fn same_layout(&self, data: &[u8]) -> bool {
        data.get(0..4) == self.file.data.get(0..4) && super::reader::ElementsFile::parse_with(data.to_vec(), &super::markers_of(&self.file)).is_ok()
    }

    /// Digest slots of another copy of the file.
    fn slots_in(&self, data: &[u8]) -> Vec<usize> {
        super::reader::ElementsFile::parse_with(data.to_vec(), &super::markers_of(&self.file))
            .map(|f| slots_of(&f.segments))
            .unwrap_or_default()
    }

    /// What saving to `options.path` would do.
    pub fn save_plan(&self, options: &SaveOptions) -> Result<SavePlan, String> {
        let target = PathBuf::from(&options.path);
        let replaces = target.is_file();
        let same_file = same_path(&target, Path::new(&self.path));
        let state = self.edit_state();
        Ok(SavePlan {
            path: options.path.clone(),
            replaces,
            same_file,
            changed_on_disk: self.changed_on_disk(),
            read_only: replaces && std::fs::metadata(&target).is_ok_and(|m| m.permissions().readonly()),
            size: self.file.data.len(),
            changed: state.changed.len(),
            added: state.added.len(),
            deleted: state.deleted.iter().map(|(_, n)| n).sum(),
            checksum: self.check_checksum(&target, options.path_data.as_deref())?,
            backup: (options.backup && replaces && !self.backed_up.contains(&target)).then(|| backup_path(&target).display().to_string()),
        })
    }

    /// Writes the file. The open document then is that file: its path, its
    /// digest and export time, and edits count from it.
    pub fn save(&mut self, options: &SaveOptions) -> Result<SaveReport, String> {
        let target = PathBuf::from(&options.path);
        if !options.replace_changed && self.changed_on_disk() && same_path(&target, Path::new(&self.path)) {
            return Err(format!("{CHANGED_ON_DISK}: {} was changed by another program since it was read", self.path));
        }
        let mut data = self.file.data.clone();
        let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0);
        data[4..8].copy_from_slice(&timestamp.to_le_bytes());

        let slots = self.checksum_slots();
        let path_data = self.find_path_data(&target, options.path_data.as_deref());
        let digest = match (&path_data, slots.len() >= DIGEST_SLOTS) {
            (Some((p, _)), true) => {
                let bytes = std::fs::read(p).map_err(|e| format!("Could not read {}: {e}", p.display()))?;
                let d = digest(&data, &slots, &bytes);
                for (i, &slot) in slots[..DIGEST_SLOTS].iter().enumerate() {
                    data[slot..slot + 8].copy_from_slice(&d.as_bytes()[i * 8..i * 8 + 8]);
                }
                Some(d)
            }
            _ => None,
        };

        let replaces = target.is_file();
        let backup = if options.backup && replaces && !self.backed_up.contains(&target) {
            let b = backup_path(&target);
            std::fs::copy(&target, &b).map_err(|e| format!("Could not back up {} to {}: {e}", target.display(), b.display()))?;
            Some(b)
        } else {
            None
        };
        write_replacing(&target, &data)?;

        // The open document now is the saved file.
        self.file.data = data;
        self.file.timestamp = timestamp;
        self.path = target.display().to_string();
        self.disk = DiskStamp::of(&target);
        if backup.is_some() || !replaces {
            // A new file needs no backup of itself later on.
            self.backed_up.insert(target.clone());
        }
        self.edits.mark_saved(timestamp);
        Ok(SaveReport {
            path: self.path.clone(),
            size: self.file.data.len(),
            digest,
            path_data: path_data.map(|(p, _)| p.display().to_string()),
            backup: backup.map(|b| b.display().to_string()),
            timestamp,
        })
    }
}

/// Writes next to the target, then moves the new file over it, so the
/// target is either the old file or the whole new one.
fn write_replacing(target: &Path, data: &[u8]) -> Result<(), String> {
    let name = target.file_name().ok_or("The file has no name")?.to_string_lossy().into_owned();
    let tmp = target.with_file_name(format!("{name}.jdide-saving"));
    std::fs::write(&tmp, data).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
    // The official tools clear the read-only flag as well.
    if let Ok(meta) = std::fs::metadata(target) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(target, perms);
        }
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not replace {}: {e}", target.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::edit::FieldEdit;
    use crate::elements::format::Catalog;
    use std::sync::Arc;

    /// The v156 server files: written by the official editor, with a valid digest.
    const V156: &str = "E:/Game Dev/JD/zxserver/zgame/gs/config";

    fn open_v156() -> Option<Document> {
        let path = format!("{V156}/elements.data");
        if !Path::new(&path).is_file() || !Path::new(&format!("{V156}/path.data")).is_file() {
            eprintln!("skipping: {V156} not found");
            return None;
        }
        Some(Document::open(path, Arc::new(Catalog::load(None))).unwrap())
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jdide-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn digest_matches_the_official_editor() {
        let Some(doc) = open_v156() else { return };
        let slots = doc.checksum_slots();
        assert_eq!(slots.len(), 4);
        let path_data = std::fs::read(format!("{V156}/path.data")).unwrap();
        let stored = stored_digest(&doc.file.data, &slots).unwrap();
        assert_eq!(digest(&doc.file.data, &slots, &path_data), stored);
        let check = doc.check_checksum(Path::new(&doc.path), None).unwrap();
        assert_eq!(check.status, ChecksumStatus::Valid);
        assert_eq!(check.path_data_from, Some("next to the file"));
    }

    #[test]
    fn saves_edits_with_a_new_digest() {
        let Some(mut doc) = open_v156() else { return };
        let original = doc.file.data.clone();
        let dir = scratch("edits");
        let target = dir.join("elements.data");
        std::fs::copy(format!("{V156}/path.data"), dir.join("path.data")).unwrap();

        // Unchanged: the same bytes but for the export time.
        let options = |backup| SaveOptions { path: target.display().to_string(), path_data: None, backup, replace_changed: false };
        let plan = doc.save_plan(&options(true)).unwrap();
        assert!(!plan.replaces && !plan.same_file && plan.backup.is_none());
        let report = doc.save(&options(true)).unwrap();
        let saved = std::fs::read(&target).unwrap();
        assert_eq!(saved.len(), original.len());
        // The time is hashed too, so only the digest slots differ.
        let slots = doc.checksum_slots();
        let masked = |d: &[u8]| {
            let mut d = d.to_vec();
            for &s in slots.iter().chain([4].iter()) {
                d[s..s + if s == 4 { 4 } else { 8 }].fill(0);
            }
            d
        };
        assert!(masked(&saved) == masked(&original), "only the time and digest change");
        assert_eq!(report.digest, stored_digest(&saved, &slots));
        assert_eq!(doc.check_checksum(&target, None).unwrap().status, ChecksumStatus::Valid);
        assert_eq!(doc.path, target.display().to_string());

        // An edit and a clone: saved, re-read, digest valid for the new bytes.
        let row = doc.records(3).unwrap().into_iter().find(|r| r.id > 1000).unwrap();
        let price = doc.record(3, row.index).unwrap().nodes.into_iter().find(|n| n.name == "price").unwrap();
        doc.edit(3, row.index, &[FieldEdit { off: price.off, value: "4242".into() }], "Set price").unwrap();
        let cloned = doc.clone_record(3, row.index).unwrap().created.unwrap();
        let count = doc.file.lists[3].count;
        let plan = doc.save_plan(&options(true)).unwrap();
        assert!(plan.replaces && plan.same_file && !plan.changed_on_disk);
        assert_eq!((plan.changed, plan.added), (1, 1));
        assert!(plan.backup.is_none(), "a file this session made needs no backup");
        let report = doc.save(&options(true)).unwrap();
        assert!(report.backup.is_none());

        let reread = Document::open(target.display().to_string(), Arc::new(Catalog::load(None))).unwrap();
        assert!(reread.file.data == doc.file.data, "the document holds what was written");
        assert_eq!(reread.file.lists[3].count, count);
        let price_now = reread.record(3, row.index).unwrap().nodes.into_iter().find(|n| n.name == "price").unwrap();
        assert_eq!(price_now.value.as_deref(), Some("4242"));
        assert_eq!(reread.record(3, cloned.1).unwrap().nodes[0].value, doc.record(3, cloned.1).unwrap().nodes[0].value);
        let check = reread.check_checksum(&target, None).unwrap();
        assert_eq!(check.status, ChecksumStatus::Valid, "{check:?}");
        assert_eq!(report.digest, check.stored);

        // Edits now count from the saved file; undo still goes back past it.
        let state = doc.edit_state();
        assert!(state.changed.is_empty() && state.added.is_empty() && state.deleted.is_empty());
        doc.undo();
        assert_eq!(doc.edit_state().deleted, vec![(3, 1)], "undoing the clone removes a saved record");
        doc.redo();
        assert!(doc.edit_state().deleted.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backs_up_once_and_notices_changes_on_disk() {
        let Some(source) = open_v156() else { return };
        let dir = scratch("backup");
        let target = dir.join("elements.data");
        std::fs::write(&target, &source.file.data).unwrap();
        let mut doc = Document::open(target.display().to_string(), Arc::new(Catalog::load(None))).unwrap();
        // No path.data anywhere near: saved without a digest.
        let options = SaveOptions { path: target.display().to_string(), path_data: None, backup: true, replace_changed: false };
        let plan = doc.save_plan(&options).unwrap();
        assert_eq!(plan.checksum.status, ChecksumStatus::NoPathData);
        assert!(plan.backup.is_some());
        let report = doc.save(&options).unwrap();
        let backup = PathBuf::from(report.backup.unwrap());
        assert_eq!(std::fs::read(&backup).unwrap(), source.file.data);
        assert!(report.digest.is_none());
        // The second save keeps the first backup.
        assert!(doc.save(&options).unwrap().backup.is_none());
        // A chosen path.data gives the digest.
        let with = SaveOptions { path_data: Some(format!("{V156}/path.data")), ..options };
        assert_eq!(doc.save_plan(&with).unwrap().checksum.path_data_from, Some("chosen"));
        assert!(doc.save(&with).unwrap().digest.is_some());
        assert_eq!(doc.check_checksum(&target, with.path_data.as_deref()).unwrap().status, ChecksumStatus::Valid);
        // Someone else writes the file.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&target, &source.file.data[..source.file.data.len() - 1]).unwrap();
        assert!(doc.save_plan(&with).unwrap().changed_on_disk);
        assert!(doc.save(&with).unwrap_err().starts_with(CHANGED_ON_DISK));
        assert!(doc.save(&SaveOptions { replace_changed: true, ..with }).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
