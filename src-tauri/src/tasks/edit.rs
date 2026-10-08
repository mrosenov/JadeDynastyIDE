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
}

#[derive(Debug, Clone, Default)]
pub struct Journal {
    done: Vec<Entry>,
    undone: Vec<Entry>,
    next_id: u64,
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

    pub fn record(&mut self, details: EntryDetails, changes: Vec<RootChange>) {
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
        });
        self.undone.clear();
    }

    pub fn state(&self, mut changed_roots: Vec<ChangedRoot>) -> EditState {
        changed_roots.sort_by_key(|root| (root.pack, root.root));
        EditState {
            undo: self.done.last().map(|entry| entry.label.clone()),
            redo: self.undone.last().map(|entry| entry.label.clone()),
            changed_roots,
        }
    }

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.done.iter().map(|entry| history(entry, false))
            .chain(self.undone.iter().rev().map(|entry| history(entry, true)))
            .collect()
    }

    pub fn undo(&mut self) -> Option<Vec<RootChange>> {
        let entry = self.done.pop()?;
        let changes = entry.changes.iter().rev().map(|change| RootChange {
            pack: change.pack,
            root: change.root,
            before: change.after.clone(),
            after: change.before.clone(),
        }).collect();
        self.undone.push(entry);
        Some(changes)
    }

    pub fn redo(&mut self) -> Option<Vec<RootChange>> {
        let entry = self.undone.pop()?;
        let changes = entry.changes.clone();
        self.done.push(entry);
        Some(changes)
    }
}

fn history(entry: &Entry, undone: bool) -> HistoryEntry {
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
    }
}
