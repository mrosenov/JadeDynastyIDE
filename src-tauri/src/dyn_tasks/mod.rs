//! The `dyn_tasks.data` workspace: an opened pack, its edits (undo/redo/history), clone, delete,
//! problems and saving. Untouched tasks keep their bytes; an edited task is written again and must
//! read back as exactly the task that was sent.

pub mod format;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use format::{AwardLayout, DynTask, Header};

/// NPC functions whose option parameter is a quest ID (give, complete, give item, give up, talk).
const QUEST_FUNCTIONS: [u32; 5] = [0, 6, 7, 8, 21];
const FUNCTION: u32 = 0x8000_0000;

#[derive(Clone)]
struct Entry {
    uid: u64,
    task: DynTask,
    bytes: Vec<u8>,
}

#[derive(Clone)]
enum Change {
    Replace { index: usize, before: Entry, after: Entry },
    Insert { index: usize, entry: Entry },
    Remove { index: usize, entry: Entry },
}

struct JournalEntry {
    id: u64,
    label: String,
    time: i64,
    task_id: u32,
    task_name: String,
    changes: Vec<Change>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynRow {
    pub index: usize,
    pub uid: u64,
    pub id: u32,
    pub name: String,
    pub dyn_type: u8,
    pub special_award: u32,
    pub method: u8,
    pub subtasks: usize,
    /// "", "changed" or "added" (since the file was opened or saved).
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynHistoryEntry {
    pub id: u64,
    pub label: String,
    pub time: i64,
    pub task_id: u32,
    pub task_name: String,
    pub undone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynView {
    pub path: String,
    pub size: usize,
    pub time_mark: i32,
    pub version: u16,
    pub layout: AwardLayout,
    pub rows: Vec<DynRow>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Done entries oldest first, then undone ones.
    pub history: Vec<DynHistoryEntry>,
    /// The journal position of the last save (or opening): entries up to it are saved.
    pub saved_entries: Option<usize>,
    pub last_saved: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynProblem {
    /// "error" or "warning".
    pub severity: &'static str,
    pub index: usize,
    pub task_id: u32,
    pub task_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynSaveReport {
    pub path: String,
    pub size: usize,
    pub tasks: usize,
    pub time_mark: i32,
    pub backup: Option<String>,
}

pub struct DynDocument {
    pub path: PathBuf,
    header: Header,
    pub layout: AwardLayout,
    entries: Vec<Entry>,
    /// Bytes of every task as last opened or saved, by uid.
    originals: HashMap<u64, Vec<u8>>,
    next_uid: u64,
    done: Vec<JournalEntry>,
    undone: Vec<JournalEntry>,
    next_entry: u64,
    saved_entries: Option<usize>,
    last_saved: Option<i64>,
    /// MD5 of the file as opened or saved (changed-on-disk guard).
    disk: String,
    backed_up: bool,
    /// The highest ID cloned or deleted this session; fresh IDs stay above it.
    id_floor: u32,
}

fn digest(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(0)
}

impl DynDocument {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let data = std::fs::read(&path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let pack = format::read(&data)?;
        let mut document = DynDocument {
            path,
            header: pack.header,
            layout: pack.layout,
            entries: Vec::new(),
            originals: HashMap::new(),
            next_uid: 1,
            done: Vec::new(),
            undone: Vec::new(),
            next_entry: 1,
            saved_entries: Some(0),
            last_saved: None,
            disk: digest(&data),
            backed_up: false,
            id_floor: 0,
        };
        for (task, bytes) in pack.tasks {
            let uid = document.next_uid;
            document.next_uid += 1;
            document.originals.insert(uid, bytes.clone());
            document.entries.push(Entry { uid, task, bytes });
        }
        Ok(document)
    }

    fn data(&self, time_mark: i32) -> Result<Vec<u8>, String> {
        let parts: Vec<&[u8]> = self.entries.iter().map(|entry| entry.bytes.as_slice()).collect();
        format::write(time_mark, &parts)
    }

    pub fn view(&self) -> DynView {
        let rows = self.entries.iter().enumerate().map(|(index, entry)| DynRow {
            index,
            uid: entry.uid,
            id: entry.task.id,
            name: entry.task.name.clone(),
            dyn_type: entry.task.dyn_type,
            special_award: entry.task.special_award,
            method: entry.task.goal.method,
            subtasks: entry.task.walk().len() - 1,
            status: match self.originals.get(&entry.uid) {
                None => "added",
                Some(bytes) if *bytes != entry.bytes => "changed",
                Some(_) => "",
            },
        }).collect();
        let entry_view = |entry: &JournalEntry, undone: bool| DynHistoryEntry { id: entry.id, label: entry.label.clone(), time: entry.time, task_id: entry.task_id, task_name: entry.task_name.clone(), undone };
        let mut history: Vec<DynHistoryEntry> = self.done.iter().map(|entry| entry_view(entry, false)).collect();
        history.extend(self.undone.iter().rev().map(|entry| entry_view(entry, true)));
        DynView {
            path: self.path.display().to_string(),
            size: format::HEADER_SIZE + self.entries.iter().map(|entry| entry.bytes.len()).sum::<usize>(),
            time_mark: self.header.time_mark,
            version: self.header.version,
            layout: self.layout,
            rows,
            dirty: self.saved_entries != Some(self.done.len()),
            can_undo: !self.done.is_empty(),
            can_redo: !self.undone.is_empty(),
            history,
            saved_entries: self.saved_entries,
            last_saved: self.last_saved,
        }
    }

    fn entry(&self, index: usize, uid: u64) -> Result<&Entry, String> {
        self.entries.get(index).filter(|entry| entry.uid == uid).ok_or_else(|| "The task list changed; select the task again".to_string())
    }

    pub fn task(&self, index: usize, uid: u64) -> Result<DynTask, String> {
        Ok(self.entry(index, uid)?.task.clone())
    }

    fn apply(&mut self, change: &Change, forward: bool) {
        match (change, forward) {
            (Change::Replace { index, after, .. }, true) => self.entries[*index] = after.clone(),
            (Change::Replace { index, before, .. }, false) => self.entries[*index] = before.clone(),
            (Change::Insert { index, entry }, true) | (Change::Remove { index, entry }, false) => self.entries.insert(*index, entry.clone()),
            (Change::Insert { index, .. }, false) | (Change::Remove { index, .. }, true) => { self.entries.remove(*index); }
        }
    }

    fn record(&mut self, label: &str, task: &DynTask, changes: Vec<Change>) {
        for change in &changes {
            self.apply(change, true);
        }
        // A new edit after undoing past the save makes the saved state unreachable.
        if self.saved_entries.is_some_and(|saved| saved > self.done.len()) {
            self.saved_entries = None;
        }
        self.undone.clear();
        self.done.push(JournalEntry { id: self.next_entry, label: label.into(), time: now(), task_id: task.id, task_name: task.name.clone(), changes });
        self.next_entry += 1;
    }

    pub fn undo(&mut self) -> Result<DynView, String> {
        let entry = self.done.pop().ok_or("Nothing to undo")?;
        for change in entry.changes.iter().rev() {
            self.apply(change, false);
        }
        self.undone.push(entry);
        Ok(self.view())
    }

    pub fn redo(&mut self) -> Result<DynView, String> {
        let entry = self.undone.pop().ok_or("Nothing to redo")?;
        for change in &entry.changes {
            self.apply(change, true);
        }
        self.done.push(entry);
        Ok(self.view())
    }

    /// IDs used by the pack (every task and subtask), optionally without one top-level task.
    fn used_ids(&self, except: Option<usize>) -> HashSet<u32> {
        self.entries.iter().enumerate().filter(|(index, _)| Some(*index) != except).flat_map(|(_, entry)| entry.task.walk().into_iter().map(|task| task.id)).collect()
    }

    /// Replaces a top-level task (with its subtasks). `quests` are the open tasks.data's IDs.
    pub fn set_task(&mut self, index: usize, uid: u64, task: DynTask, label: &str, quests: Option<&HashSet<u32>>) -> Result<DynView, String> {
        let before = self.entry(index, uid)?.clone();
        let mut seen = HashSet::new();
        let others = self.used_ids(Some(index));
        for node in task.walk() {
            if !seen.insert(node.id) {
                return Err(format!("Task ID {} is used twice in this task", node.id));
            }
            if others.contains(&node.id) {
                return Err(format!("Task ID {} is already used by another dynamic task", node.id));
            }
            let renamed = !before.task.walk().iter().any(|old| old.id == node.id);
            if renamed && quests.is_some_and(|quests| quests.contains(&node.id)) {
                return Err(format!("Task ID {} is used by the open tasks.data; the game loads both into one list", node.id));
            }
        }
        let bytes = format::write_task_bytes(&task, self.layout)?;
        if bytes == before.bytes {
            return Ok(self.view());
        }
        let after = Entry { uid: before.uid, task: format::read(&format::write(0, &[&bytes])?)?.tasks.remove(0).0, bytes };
        self.record(label, &after.task.clone(), vec![Change::Replace { index, before, after }]);
        Ok(self.view())
    }

    /// Copies a top-level task below it with fresh IDs (above this pack, the open tasks.data and
    /// every ID cloned or deleted this session) and the next free special award number.
    pub fn clone_task(&mut self, index: usize, uid: u64, quests: Option<&HashSet<u32>>) -> Result<(DynView, usize), String> {
        let source = self.entry(index, uid)?.task.clone();
        let mut next = self.used_ids(None).into_iter().chain(quests.into_iter().flatten().copied()).max().unwrap_or(0).max(self.id_floor) + 1;
        let mut map = HashMap::new();
        for node in source.walk() {
            map.insert(node.id, next);
            next += 1;
        }
        let mut task = source.clone();
        fn remap(task: &mut DynTask, map: &HashMap<u32, u32>) {
            task.id = map[&task.id];
            for list in [&mut task.premise_tasks, &mut task.mutex_tasks].into_iter().flatten() {
                for id in list.iter_mut() {
                    if let Some(new) = map.get(id) { *id = *new; }
                }
            }
            for talk in &mut task.talks {
                for window in &mut talk.windows {
                    for option in &mut window.options {
                        if option.id & FUNCTION != 0 && QUEST_FUNCTIONS.contains(&(option.id & !FUNCTION)) {
                            if let Some(new) = map.get(&option.param) { option.param = *new; }
                        }
                    }
                }
            }
            for child in &mut task.subtasks { remap(child, map); }
        }
        remap(&mut task, &map);
        if task.dyn_type == format::TYPE_SPECIAL_AWARD {
            task.special_award = self.entries.iter().map(|entry| entry.task.special_award).max().unwrap_or(0) + 1;
        }
        let bytes = format::write_task_bytes(&task, self.layout)?;
        let uid = self.next_uid;
        self.next_uid += 1;
        self.id_floor = self.id_floor.max(next - 1);
        let label = format!("Clone task {}", source.id);
        let position = index + 1;
        self.record(&label, &task.clone(), vec![Change::Insert { index: position, entry: Entry { uid, task, bytes } }]);
        Ok((self.view(), position))
    }

    pub fn delete_task(&mut self, index: usize, uid: u64) -> Result<DynView, String> {
        let entry = self.entry(index, uid)?.clone();
        self.id_floor = self.id_floor.max(entry.task.walk().iter().map(|task| task.id).max().unwrap_or(0));
        let label = format!("Delete task {}", entry.task.id);
        let task = entry.task.clone();
        self.record(&label, &task, vec![Change::Remove { index, entry }]);
        Ok(self.view())
    }

    /// Item and monster IDs the tasks use (for checking them against elements.data).
    pub fn element_ids(&self) -> HashSet<u32> {
        let mut ids = HashSet::new();
        for entry in &self.entries {
            for task in entry.task.walk() {
                let items = task.premise_items.iter().flatten()
                    .chain(task.given_items.iter().flat_map(|given| given.items.iter()))
                    .chain(task.goal.items.iter())
                    .chain(task.award.candidates.iter().flatten().flat_map(|candidate| candidate.items.iter()));
                ids.extend(items.map(|item| item.item_id).filter(|id| *id != 0));
                ids.extend(task.goal.monsters.iter().flat_map(|monster| [monster.monster_id, monster.drop_item_id]).filter(|id| *id != 0));
            }
        }
        ids
    }

    /// `quests`: the open tasks.data's IDs; `missing`: element IDs elements.data does not have.
    pub fn problems(&self, quests: Option<&HashSet<u32>>, missing: Option<&HashSet<u32>>) -> Vec<DynProblem> {
        let mut out = Vec::new();
        let mut owners: HashMap<u32, Vec<usize>> = HashMap::new();
        for (index, entry) in self.entries.iter().enumerate() {
            for task in entry.task.walk() {
                owners.entry(task.id).or_default().push(index);
            }
        }
        let known: HashSet<u32> = owners.keys().copied().chain(quests.into_iter().flatten().copied()).collect();
        for (index, entry) in self.entries.iter().enumerate() {
            for task in entry.task.walk() {
                let mut add = |severity: &'static str, message: String| out.push(DynProblem { severity, index, task_id: task.id, task_name: task.name.clone(), message });
                if owners[&task.id].len() > 1 {
                    add("error", format!("ID {} is used by {} dynamic tasks", task.id, owners[&task.id].len()));
                }
                if quests.is_some_and(|quests| quests.contains(&task.id)) {
                    add("error", format!("ID {} is also a task in the open tasks.data; the game loads both into one list", task.id));
                }
                if task.level_max != 0 && task.level_min > task.level_max {
                    add("warning", format!("The level range {}–{} is empty", task.level_min, task.level_max));
                }
                if quests.is_some() {
                    for id in task.premise_tasks.iter().flatten().chain(task.mutex_tasks.iter().flatten()) {
                        if *id != 0 && !known.contains(id) {
                            add("warning", format!("Refers to task {id}, which neither file has"));
                        }
                    }
                }
                if let Some(missing) = missing {
                    let mut ids: Vec<u32> = Vec::new();
                    ids.extend(task.premise_items.iter().flatten().map(|item| item.item_id));
                    ids.extend(task.given_items.iter().flat_map(|given| given.items.iter().map(|item| item.item_id)));
                    ids.extend(task.goal.items.iter().map(|item| item.item_id));
                    ids.extend(task.goal.monsters.iter().flat_map(|monster| [monster.monster_id, monster.drop_item_id]));
                    ids.extend(task.award.candidates.iter().flatten().flat_map(|candidate| candidate.items.iter().map(|item| item.item_id)));
                    ids.sort_unstable();
                    ids.dedup();
                    for id in ids.into_iter().filter(|id| *id != 0 && missing.contains(id)) {
                        add("warning", format!("Item or monster {id} is not in the open elements.data"));
                    }
                }
            }
        }
        out
    }

    /// Writes the pack with a new time mark (clients download it again from the server).
    pub fn save(&mut self, target: Option<&str>, backup: bool, replace_changed: bool) -> Result<DynSaveReport, String> {
        let target = target.map(PathBuf::from).unwrap_or_else(|| self.path.clone());
        let same = crate::path_data::same_path(&target, &self.path);
        if same && !replace_changed {
            if let Ok(current) = std::fs::read(&target) {
                if digest(&current) != self.disk {
                    return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", target.display()));
                }
            }
        }
        // A new time mark makes clients replace their cached copy; keep it moving forward.
        let time_mark = (now() as i32).max(self.header.time_mark.saturating_add(1));
        let data = self.data(time_mark)?;
        // The saved file must read back as exactly these tasks.
        let check = format::read(&data)?;
        if check.tasks.len() != self.entries.len() || check.tasks.iter().zip(&self.entries).any(|((_, bytes), entry)| *bytes != entry.bytes) {
            return Err("The saved pack would not read back as these tasks; nothing was written".into());
        }
        let backup_path = if backup && target.is_file() && (!same || !self.backed_up) {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "dyn_tasks.data".into());
            let path = target.with_file_name(format!("{name}.{stamp}.bak"));
            std::fs::copy(&target, &path).map_err(|error| format!("Could not back up {} to {}: {error}", target.display(), path.display()))?;
            Some(path)
        } else {
            None
        };
        crate::path_data::write_replacing(&target, &data)?;
        if same && backup_path.is_some() {
            self.backed_up = true;
        }
        self.path = target.clone();
        self.header = check.header;
        self.disk = digest(&data);
        self.originals = self.entries.iter().map(|entry| (entry.uid, entry.bytes.clone())).collect();
        self.saved_entries = Some(self.done.len());
        self.last_saved = Some(now());
        Ok(DynSaveReport { path: target.display().to_string(), size: data.len(), tasks: self.entries.len(), time_mark, backup: backup_path.map(|path| path.display().to_string()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Option<PathBuf> {
        format::tests::SAMPLES.iter().map(PathBuf::from).find(|path| path.is_file() && path.to_string_lossy().contains("ForsakenJD"))
    }

    #[test]
    fn edits_clone_delete_undo_and_save_round_trip() {
        let Some(path) = sample() else { return };
        let copy = std::env::temp_dir().join(format!("jdide-dyn-{}.data", std::process::id()));
        std::fs::copy(&path, &copy).unwrap();
        let mut document = DynDocument::open(&copy).unwrap();
        let original = std::fs::read(&copy).unwrap();
        let count = document.entries.len();

        // An edit changes one task; undo restores the exact bytes.
        let uid = document.entries[0].uid;
        let mut task = document.task(0, uid).unwrap();
        task.level_min = 33;
        task.time_limit = Some(600);
        let view = document.set_task(0, uid, task, "Edit level", None).unwrap();
        assert!(view.dirty && view.rows[0].status == "changed");
        document.undo().unwrap();
        assert_eq!(document.data(document.header.time_mark).unwrap(), original);
        document.redo().unwrap();

        // Clone gets fresh IDs and the next special award number; IDs cannot be reused by edits.
        let quests: HashSet<u32> = [40_000].into_iter().collect();
        let (view, position) = document.clone_task(1, document.entries[1].uid, Some(&quests)).unwrap();
        assert_eq!(view.rows.len(), count + 1);
        assert_eq!(view.rows[position].id, 40_001);
        assert_eq!(view.rows[position].status, "added");
        let taken = document.entries[position + 1].task.id;
        let mut clash = document.task(position, document.entries[position].uid).unwrap();
        clash.id = taken;
        assert!(document.set_task(position, document.entries[position].uid, clash, "Change ID", Some(&quests)).is_err());

        // Delete, save, reopen.
        document.delete_task(5, document.entries[5].uid).unwrap();
        let report = document.save(None, false, false).unwrap();
        assert!(report.time_mark > document.header.time_mark - 1);
        let reopened = DynDocument::open(&copy).unwrap();
        assert_eq!(reopened.entries.len(), count);
        assert_eq!(reopened.entries[0].task.level_min, 33);
        assert_eq!(reopened.entries[0].task.time_limit, Some(600));
        assert!(reopened.entries.iter().any(|entry| entry.task.id == 40_001));
        assert!(!document.view().dirty);
        let _ = std::fs::remove_file(&copy);
    }

    #[test]
    fn real_packs_have_no_errors() {
        for path in format::tests::SAMPLES {
            let Ok(document) = DynDocument::open(path) else { continue };
            let errors: Vec<_> = document.problems(None, None).into_iter().filter(|problem| problem.severity == "error").collect();
            assert!(errors.is_empty(), "{path}: {errors:?}");
        }
    }
}
