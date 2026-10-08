//! Validated multi-file saving for tasks.data and its numbered packs.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::browser::TaskDocument;
use super::container::TaskContainer;
use super::schema::decode_exact;

const INDEX_HEADER: usize = 20;
const PACK_HEADER: usize = 8;
pub const CHANGED_ON_DISK: &str = "CHANGED_ON_DISK";

#[derive(Debug, Clone, PartialEq)]
pub struct DiskStamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl DiskStamp {
    pub fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self { len: metadata.len(), modified: metadata.modified().ok() })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOptions {
    pub path: String,
    #[serde(default)]
    pub backup: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlan {
    pub path: String,
    pub replaces: bool,
    pub same_file: bool,
    pub changed_on_disk: bool,
    pub read_only: bool,
    pub changed_roots: usize,
    pub changed_packs: usize,
    pub pack_count: usize,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: u64,
    pub changed_roots: usize,
    pub changed_packs: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
}

pub fn stamps(container: &TaskContainer) -> HashMap<PathBuf, DiskStamp> {
    std::iter::once(container.index_path()).chain(container.packs.iter().map(|pack| pack.path()))
        .filter_map(|path| DiskStamp::of(path).map(|stamp| (path.to_path_buf(), stamp)))
        .collect()
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn numbered(index: &Path, number: usize) -> PathBuf {
    let mut name: OsString = index.as_os_str().to_owned();
    name.push(number.to_string());
    PathBuf::from(name)
}

fn backup_path(target: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "tasks.data".into());
    target.with_file_name(format!("{name}.{stamp}.bak"))
}

fn set_writable(path: &Path) {
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(path, permissions);
        }
    }
}

fn replace_file(staged: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        set_writable(target);
    }
    std::fs::rename(staged, target).map_err(|error| format!("Could not replace {}: {error}", target.display()))
}

fn restore_files(files: &[(PathBuf, PathBuf)], rollback: &Path) {
    for (_, target) in files {
        let saved = rollback.join(target.file_name().unwrap());
        if saved.is_file() {
            set_writable(target);
            let _ = std::fs::copy(saved, target);
        } else if target.is_file() {
            set_writable(target);
            let _ = std::fs::remove_file(target);
        }
    }
}

impl TaskDocument {
    fn changed_on_disk(&self) -> bool {
        self.disk.iter().any(|(path, stamp)| DiskStamp::of(path) != Some(stamp.clone()))
    }

    pub fn save_plan(&self, options: &SaveOptions) -> Result<SavePlan, String> {
        let target = PathBuf::from(&options.path);
        let same_file = same_path(&target, self.container.index_path());
        if target.is_file() && !same_file {
            TaskContainer::open(&target).map_err(|error| format!("The existing destination is not a valid task set: {error}"))?;
        }
        let changed_packs = self.changed_root_keys().into_iter().map(|(pack, _)| pack).collect::<HashSet<_>>().len();
        let read_only = std::iter::once(target.clone()).chain((1..=self.container.packs.len()).map(|number| numbered(&target, number)))
            .any(|path| std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().readonly()));
        Ok(SavePlan {
            path: options.path.clone(),
            replaces: target.is_file(),
            same_file,
            changed_on_disk: self.changed_on_disk(),
            read_only,
            changed_roots: self.changed_root_keys().len(),
            changed_packs,
            pack_count: self.container.packs.len(),
            size: self.summary.size,
            backup: (options.backup && target.is_file() && !self.backed_up.contains(&target)).then(|| backup_path(&target).display().to_string()),
        })
    }

    pub fn save(&mut self, options: &SaveOptions) -> Result<SaveReport, String> {
        let target = PathBuf::from(&options.path);
        let same_file = same_path(&target, self.container.index_path());
        if self.changed_on_disk() {
            return Err(format!("{CHANGED_ON_DISK}: the task index or one of its packs changed after it was opened; reopen the task set before saving"));
        }
        let parent = target.parent().ok_or("The target has no parent folder")?;
        std::fs::create_dir_all(parent).map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        let stage = parent.join(format!(".jdide-tasks-saving-{}", std::process::id()));
        if stage.exists() {
            std::fs::remove_dir_all(&stage).map_err(|error| format!("Could not clear {}: {error}", stage.display()))?;
        }
        std::fs::create_dir(&stage).map_err(|error| format!("Could not create {}: {error}", stage.display()))?;
        let staged_index = stage.join(target.file_name().ok_or("The target has no file name")?);
        let changed_packs = self.changed_root_keys().into_iter().map(|(pack, _)| pack).collect::<HashSet<_>>();
        let changed_roots = self.changed_root_keys().len();
        let had_structural_roots = !self.added_roots.is_empty();
        let replaces = target.is_file();
        let result = self.stage(&staged_index, &changed_packs)
            .and_then(|_| self.validate_staged(&staged_index))
            .and_then(|_| self.install(&staged_index, &target, &changed_packs, same_file, options.backup));
        let _ = std::fs::remove_dir_all(&stage);
        let backup = result?;

        let container = TaskContainer::open(&target)?;
        let size = std::fs::metadata(&target).map(|metadata| metadata.len()).unwrap_or(0) + container.total_pack_bytes();
        self.container = container;
        self.summary.path = target.display().to_string();
        self.summary.size = size;
        self.disk = stamps(&self.container);
        self.modified.clear();
        self.added_roots.clear();
        if had_structural_roots {
            self.journal.clear();
        }
        if let Some(cache) = self.cache.as_mut() {
            cache.original = cache.node.clone();
        }
        if backup.is_some() || !replaces {
            self.backed_up.insert(target.clone());
        }
        Ok(SaveReport {
            path: target.display().to_string(),
            size,
            changed_roots,
            changed_packs: changed_packs.len(),
            backup: backup.map(|path| path.display().to_string()),
        })
    }

    fn stage(&self, target: &Path, changed_packs: &HashSet<usize>) -> Result<(), String> {
        let mut index = std::fs::read(self.container.index_path()).map_err(|error| format!("Could not read task index: {error}"))?;
        let root_count = u32::try_from(self.summary.root_count).map_err(|_| "Task root count exceeds the 32-bit limit")?;
        index[12..16].copy_from_slice(&root_count.to_le_bytes());
        for (pack_index, pack) in self.container.packs.iter().enumerate() {
            let staged = numbered(target, pack_index + 1);
            if changed_packs.contains(&pack_index) {
                let count = self.root_count(pack_index)?;
                let header_size = PACK_HEADER.checked_add(count.checked_mul(4).ok_or("Task pack header overflow")?).ok_or("Task pack header overflow")?;
                let mut output = Vec::with_capacity(pack.size as usize);
                output.extend_from_slice(&0x0693_4554u32.to_le_bytes());
                output.extend_from_slice(&(count as u32).to_le_bytes());
                output.resize(header_size, 0);
                for root in 0..count {
                    let offset = u32::try_from(output.len()).map_err(|_| "Task pack exceeds the 32-bit offset limit")?;
                    output[PACK_HEADER + root * 4..PACK_HEADER + root * 4 + 4].copy_from_slice(&offset.to_le_bytes());
                    output.extend_from_slice(&self.current_root(pack_index, root)?);
                }
                std::fs::write(&staged, &output).map_err(|error| format!("Could not write {}: {error}", staged.display()))?;
            } else {
                std::fs::copy(pack.path(), &staged).map_err(|error| format!("Could not stage {}: {error}", pack.path().display()))?;
            }
            let digest: [u8; 16] = Md5::digest(std::fs::read(&staged).map_err(|error| error.to_string())?).into();
            let at = INDEX_HEADER + pack_index * 16;
            index[at..at + 16].copy_from_slice(&digest);
        }
        std::fs::write(target, index).map_err(|error| format!("Could not write {}: {error}", target.display()))
    }

    fn validate_staged(&self, target: &Path) -> Result<(), String> {
        let staged = TaskContainer::open(target)?;
        for (pack_index, pack) in staged.packs.iter().enumerate() {
            for root in 0..pack.root_count() {
                let bytes = staged.root(pack_index, root)?;
                let decoded = decode_exact(&self.schema, &bytes, staged.header.version)
                    .map_err(|error| format!("Staged tasks.data{} root {}: {error}", pack_index + 1, root + 1))?;
                if decoded.encode()? != bytes {
                    return Err(format!("Staged tasks.data{} root {} failed its exact byte round trip", pack_index + 1, root + 1));
                }
            }
        }
        Ok(())
    }

    fn install(&mut self, staged: &Path, target: &Path, changed_packs: &HashSet<usize>, same_file: bool, backup: bool) -> Result<Option<PathBuf>, String> {
        let replaced_pack_count = if target.is_file() { TaskContainer::open(target)?.packs.len() } else { 0 };
        let persistent_backup = if backup && target.is_file() && !self.backed_up.contains(target) {
            let folder = backup_path(target);
            std::fs::create_dir(&folder).map_err(|error| format!("Could not create backup {}: {error}", folder.display()))?;
            std::fs::copy(target, folder.join(target.file_name().unwrap())).map_err(|error| error.to_string())?;
            for number in 1..=replaced_pack_count {
                let source = numbered(target, number);
                if source.is_file() {
                    std::fs::copy(&source, folder.join(source.file_name().unwrap())).map_err(|error| error.to_string())?;
                }
            }
            Some(folder)
        } else { None };
        let rollback = staged.parent().unwrap().join("rollback");
        std::fs::create_dir(&rollback).map_err(|error| error.to_string())?;
        let all_packs = !same_file;
        let mut files = (0..self.container.packs.len()).filter(|pack| all_packs || changed_packs.contains(pack))
            .map(|pack| (numbered(staged, pack + 1), numbered(target, pack + 1))).collect::<Vec<_>>();
        files.push((staged.to_path_buf(), target.to_path_buf()));
        for (_, path) in &files {
            if path.is_file() {
                std::fs::copy(path, rollback.join(path.file_name().unwrap())).map_err(|error| format!("Could not prepare rollback for {}: {error}", path.display()))?;
            }
        }
        for (source, path) in &files {
            if let Err(error) = replace_file(source, path) {
                restore_files(&files, &rollback);
                return Err(error);
            }
        }
        if let Err(error) = TaskContainer::open(target) {
            restore_files(&files, &rollback);
            return Err(format!("Installed task set failed validation and was rolled back: {error}"));
        }
        Ok(persistent_backup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::browser::{FieldEdit, FieldView};

    fn fixture() -> Option<(PathBuf, PathBuf)> {
        let source = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source.is_file() { return None }
        let source = TaskContainer::open(source).unwrap();
        let roots = [source.root(0, 0).unwrap(), source.root(0, 1).unwrap()];
        Some(write_fixture(&roots, source.header.export_version))
    }

    fn write_fixture(roots: &[Vec<u8>], export_version: u32) -> (PathBuf, PathBuf) {
        let folder = std::env::temp_dir().join(format!("jdide-task-save-{}-{}", std::process::id(), SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&folder).unwrap();
        let index = folder.join("tasks.data");
        let pack = numbered(&index, 1);
        let mut pack_bytes = Vec::new();
        pack_bytes.extend_from_slice(&0x0693_4554u32.to_le_bytes());
        pack_bytes.extend_from_slice(&(roots.len() as u32).to_le_bytes());
        let header = 8 + roots.len() * 4;
        let mut offset = header;
        for root in roots {
            pack_bytes.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += root.len();
        }
        for root in roots { pack_bytes.extend_from_slice(root) }
        std::fs::write(&pack, &pack_bytes).unwrap();
        let digest: [u8; 16] = Md5::digest(&pack_bytes).into();
        let mut index_bytes = Vec::new();
        index_bytes.extend_from_slice(&0x6934_0304u32.to_le_bytes());
        index_bytes.extend_from_slice(&165u32.to_le_bytes());
        index_bytes.extend_from_slice(&export_version.to_le_bytes());
        index_bytes.extend_from_slice(&(roots.len() as u32).to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&digest);
        std::fs::write(&index, index_bytes).unwrap();
        (folder, index)
    }

    #[test]
    fn no_edit_copy_is_identical_and_one_pack_edit_is_valid() {
        let Some((folder, index)) = fixture() else { return };
        let mut document = TaskDocument::open(&index).unwrap();
        let original_index = std::fs::read(&index).unwrap();
        let original_pack = std::fs::read(numbered(&index, 1)).unwrap();
        let copy = folder.join("copy.data");
        let report = document.save(&SaveOptions { path: copy.display().to_string(), backup: false }).unwrap();
        assert_eq!(report.changed_roots, 0);
        assert_eq!(std::fs::read(&copy).unwrap(), original_index);
        assert_eq!(std::fs::read(numbered(&copy, 1)).unwrap(), original_pack);

        let mut document = TaskDocument::open(&copy).unwrap();
        document.edit_field(FieldEdit { pack: 0, root: 0, task_path: Vec::new(), field_path: vec!["fixed".into(), "name".into()], value: "Saved quest".into() }).unwrap();
        let before_second = TaskContainer::open(&copy).unwrap().root(0, 1).unwrap();
        let saved = document.save(&SaveOptions { path: copy.display().to_string(), backup: true }).unwrap();
        assert_eq!(saved.changed_roots, 1);
        assert_eq!(saved.changed_packs, 1);
        assert!(saved.backup.as_ref().is_some_and(|path| Path::new(path).is_dir()));
        let reopened = TaskContainer::open(&copy).unwrap();
        assert_eq!(reopened.root(0, 1).unwrap(), before_second);
        assert_ne!(std::fs::read(&copy).unwrap()[20..36], original_index[20..36]);
        assert!(document.edit_state().changed_roots.is_empty());
        assert_eq!(document.undo().unwrap().changed_roots.len(), 1);
        assert!(document.redo().unwrap().changed_roots.is_empty());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn cloned_top_level_task_rebuilds_the_pack_table_and_index_count() {
        let Some((folder, index)) = fixture() else { return };
        let mut document = TaskDocument::open(&index).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !document.search("", 0).indexed {
            assert!(std::time::Instant::now() < deadline, "background task index timed out");
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let source = document.summary().roots[0].clone();
        let clone = document.clone_root_task(source.pack, source.root).unwrap();
        let clone_bytes = document.current_root(clone.pack, clone.root).unwrap();
        assert_eq!(document.summary().root_count, 3);
        document.save(&SaveOptions { path: index.display().to_string(), backup: false }).unwrap();

        let reopened = TaskContainer::open(&index).unwrap();
        assert_eq!(reopened.header.root_count, 3);
        assert_eq!(reopened.packs[clone.pack].root_count(), 3);
        assert_eq!(reopened.root(clone.pack, clone.root).unwrap(), clone_bytes);
        assert!(document.edit_state().undo.is_none(), "saving a structural task clone starts a new undo history");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn refuses_a_pack_changed_after_opening() {
        let Some((folder, index)) = fixture() else { return };
        let mut document = TaskDocument::open(&index).unwrap();
        let pack = numbered(&index, 1);
        let mut bytes = std::fs::read(&pack).unwrap();
        bytes.push(0);
        std::fs::write(&pack, bytes).unwrap();
        let error = document.save(&SaveOptions { path: index.display().to_string(), backup: false }).unwrap_err();
        assert!(error.starts_with(CHANGED_ON_DISK), "{error}");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn variable_text_recalculates_following_root_offsets() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        fn variable(fields: &[FieldView]) -> Option<FieldView> {
            fields.iter().find_map(|field| {
                if field.editable && matches!(field.ty.as_str(), "wstring" | "counted wstring") { Some(field.clone()) }
                else { variable(&field.children) }
            })
        }
        let mut source = TaskDocument::open(source_path).unwrap();
        let mut found = None;
        for root in source.summary().roots.into_iter().take(500) {
            let detail = source.task(root.pack, root.root, &[]).unwrap();
            if let Some(field) = variable(&detail.fields) {
                let pack = &source.container.packs[root.pack];
                if root.root + 1 < pack.root_count() {
                    found = Some((source.current_root(root.pack, root.root).unwrap(), source.current_root(root.pack, root.root + 1).unwrap(), field));
                    break;
                }
            }
        }
        let (first, second, field) = found.expect("real task set should have variable text before another root");
        let (folder, index) = write_fixture(&[first, second.clone()], source.container.header.export_version);
        let mut document = TaskDocument::open(&index).unwrap();
        let before = std::fs::read(numbered(&index, 1)).unwrap();
        let before_second_offset = u32::from_le_bytes(before[12..16].try_into().unwrap());
        let replacement = if field.value.as_deref() == Some("JD IDE saved variable text") { "x" } else { "JD IDE saved variable text" };
        document.edit_field(FieldEdit { pack: 0, root: 0, task_path: Vec::new(), field_path: field.path, value: replacement.into() }).unwrap();
        document.save(&SaveOptions { path: index.display().to_string(), backup: false }).unwrap();
        let after = std::fs::read(numbered(&index, 1)).unwrap();
        let after_second_offset = u32::from_le_bytes(after[12..16].try_into().unwrap());
        assert_ne!(after_second_offset, before_second_offset);
        assert_eq!(TaskContainer::open(&index).unwrap().root(0, 1).unwrap(), second);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
