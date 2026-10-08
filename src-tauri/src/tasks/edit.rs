//! In-memory task edits. Each history operation stores complete root bytes so
//! variable-length strings and every following offset undo exactly.

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct RootChange {
    pub pack: usize,
    pub root: usize,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Entry {
    id: u64,
    label: String,
    time: u64,
    task_id: u32,
    task_name: String,
    field: String,
    old: String,
    new: String,
    changes: Vec<RootChange>,
    /// The entry this one reverted from the history. Such entries are not
    /// listed; the reverted entry shows as reverted instead.
    reverts: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Journal {
    done: Vec<Entry>,
    undone: Vec<Entry>,
    next_id: u64,
    /// The last listed entry included in the saved file, and when (unix ms).
    saved: Option<(Option<u64>, u64)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: u64,
    pub label: String,
    pub time: u64,
    pub task_id: u32,
    pub task_name: String,
    pub field: String,
    pub old: String,
    pub new: String,
    pub undone: bool,
    /// When a later revert from the history took this entry back (unix ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverted_at: Option<u64>,
    /// The task set was last saved after this entry (unix ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<u64>,
    /// Why this entry cannot be reverted on its own right now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revert_blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedRoot {
    pub pack: usize,
    pub root: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo: Option<String>,
    pub changed_roots: Vec<ChangedRoot>,
    /// When the task set was last saved in this session (unix ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_saved: Option<u64>,
}

pub struct EntryDetails {
    pub label: String,
    pub task_id: u32,
    pub task_name: String,
    pub field: String,
    pub old: String,
    pub new: String,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_millis() as u64)
}

impl Journal {
    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }

    /// The task set was saved: the history shows a Saved line after the
    /// latest listed entry.
    pub fn mark_saved(&mut self) {
        let last = self.done.iter().rev().find(|entry| entry.reverts.is_none()).map(|entry| entry.id);
        self.saved = Some((last, now_ms()));
    }

    pub fn record(&mut self, details: EntryDetails, changes: Vec<RootChange>) {
        self.push(details, changes, None);
    }

    /// Records the revert of entry `reverts` from the history.
    pub fn record_revert(&mut self, details: EntryDetails, changes: Vec<RootChange>, reverts: u64) {
        self.push(details, changes, Some(reverts));
    }

    /// An applied entry that a revert has not taken back yet: its label and changes.
    pub fn revertable(&self, id: u64) -> Option<(String, Vec<RootChange>)> {
        if self.done.iter().any(|entry| entry.reverts == Some(id)) {
            return None;
        }
        self.done.iter().find(|entry| entry.id == id).map(|entry| (entry.label.clone(), entry.changes.clone()))
    }

    pub fn is_reverted(&self, id: u64) -> bool {
        self.done.iter().any(|entry| entry.reverts == Some(id))
    }

    /// The latest applied entry after `id` that changed the given root.
    pub fn later_change(&self, id: u64, pack: usize, root: usize) -> Option<String> {
        self.done.iter().rev().take_while(|entry| entry.id != id)
            .find(|entry| entry.changes.iter().any(|change| change.pack == pack && change.root == root))
            .map(|entry| entry.label.clone())
    }

    fn push(&mut self, details: EntryDetails, changes: Vec<RootChange>, reverts: Option<u64>) {
        self.next_id += 1;
        self.done.push(Entry {
            id: self.next_id,
            label: details.label,
            time: now_ms(),
            task_id: details.task_id,
            task_name: details.task_name,
            field: details.field,
            old: details.old,
            new: details.new,
            changes,
            reverts,
        });
        self.undone.clear();
    }

    pub fn state(&self, mut changed_roots: Vec<ChangedRoot>) -> EditState {
        changed_roots.sort_by_key(|root| (root.pack, root.root));
        EditState {
            undo: self.done.last().map(|entry| entry.label.clone()),
            redo: self.undone.last().map(|entry| entry.label.clone()),
            changed_roots,
            last_saved: self.saved.map(|(_, time)| time),
        }
    }

    /// Every listed entry, newest first: entries Redo would apply again, then
    /// applied ones. Reverts from the history are not listed; the entry they
    /// took back shows as reverted, so reverting never goes back and forth.
    pub fn history(&self) -> Vec<HistoryEntry> {
        let reverted_at = self.done.iter().filter_map(|entry| Some((entry.reverts?, entry.time))).collect::<std::collections::HashMap<_, _>>();
        let saved_at = |entry: &Entry| self.saved.and_then(|(last, time)| (last == Some(entry.id)).then_some(time));
        self.undone.iter().filter(|entry| entry.reverts.is_none()).map(|entry| history(entry, true, None, saved_at(entry)))
            .chain(self.done.iter().rev().filter(|entry| entry.reverts.is_none()).map(|entry| history(entry, false, reverted_at.get(&entry.id).copied(), saved_at(entry))))
            .collect()
    }

    /// The changes that undo the latest entry. The entry moves to the redo
    /// stack only through `commit_undo`, after the changes were applied.
    pub fn undo_changes(&self) -> Option<Vec<RootChange>> {
        let entry = self.done.last()?;
        Some(entry.changes.iter().rev().map(|change| RootChange {
            pack: change.pack,
            root: change.root,
            before: change.after.clone(),
            after: change.before.clone(),
        }).collect())
    }

    pub fn commit_undo(&mut self) {
        if let Some(entry) = self.done.pop() {
            self.undone.push(entry);
        }
    }

    pub fn redo_changes(&self) -> Option<Vec<RootChange>> {
        Some(self.undone.last()?.changes.clone())
    }

    pub fn commit_redo(&mut self) {
        if let Some(entry) = self.undone.pop() {
            self.done.push(entry);
        }
    }
}

fn history(entry: &Entry, undone: bool, reverted_at: Option<u64>, saved_at: Option<u64>) -> HistoryEntry {
    HistoryEntry {
        id: entry.id,
        label: entry.label.clone(),
        time: entry.time,
        task_id: entry.task_id,
        task_name: entry.task_name.clone(),
        field: entry.field.clone(),
        old: entry.old.clone(),
        new: entry.new.clone(),
        undone,
        reverted_at,
        saved_at,
        revert_blocked: None,
    }
}
