//! Changing a quest's ID and every reference to it in the task set.
//!
//! References come from the background index (prerequisites, exclusions, next
//! quests, finish counts, team and master/apprentice quests, dialog option
//! parameters). Each affected root is rewritten with the same remapping a clone
//! uses, the quest's own root also gets its hierarchy links refreshed, and all
//! roots change as one undo step. When another quest has the same ID, other
//! quests' references stay: they may mean that quest.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use super::browser::{rewrite_internal_task_references, sync_hierarchy_links, task_at, task_at_mut, task_heading, verify_task_root, TaskDocument};
use super::edit::{EditState, EntryDetails, RootChange};
use super::schema::{decode_exact, Value};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdReference {
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub task_id: u32,
    pub task_name: String,
    /// `fixed.premise_tasks[0]`, `dialogs.delivery.windows[0].options[1].parameter`, …
    pub field: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdChange {
    pub old_id: u32,
    pub new_id: u32,
    pub name: String,
    /// References the change rewrites (the quest's own included).
    pub references: Vec<TaskIdReference>,
    /// Other quests with the same old ID: their references are left alone.
    pub duplicates: usize,
    /// Top-level quests rewritten.
    pub roots: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EditState>,
}

impl TaskDocument {
    /// Previews (`apply` false) or applies giving the quest at `task_path` the ID `new_id`.
    /// `expected_id` guards against acting on another quest after an undo.
    pub fn change_task_id(&mut self, pack: usize, root: usize, task_path: &[usize], expected_id: u32, new_id: u32, apply: bool) -> Result<TaskIdChange, String> {
        let version = self.container.header.version;
        let before = self.current_root(pack, root)?;
        let decoded = decode_exact(&self.schema, &before, version)?;
        let (old_id, name) = task_heading(task_at(&decoded, task_path)?)?;
        if old_id != expected_id {
            return Err(format!("The selected quest is now {old_id}, not {expected_id}; select it again"));
        }
        if new_id == 0 {
            return Err("Quest ID 0 means \"no quest\"; choose another ID".into());
        }
        if new_id == old_id {
            return Err("The quest already has this ID".into());
        }
        let (references, duplicates, roots) = {
            let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
            if !index.indexed {
                return Err("Wait until every quest is indexed (the quest list shows Indexing quests…)".into());
            }
            if let Some(owner) = index.by_id.get(&new_id) {
                return Err(format!("ID {new_id} is already used by {} ({})", if owner.name.is_empty() { "a quest" } else { owner.name.as_str() }, owner.id));
            }
            let duplicates = index.entries.iter().filter(|task| task.id == old_id && !(task.pack == pack && task.root == root && task.path == task_path)).count();
            // With a duplicate ID, only references inside the quest's own root are its own for sure.
            let references: Vec<TaskIdReference> = index.entries.iter()
                .filter(|task| duplicates == 0 || (task.pack == pack && task.root == root))
                .flat_map(|task| task.references.iter().filter(|reference| !reference.element && reference.target_id == old_id).map(move |reference| TaskIdReference {
                    pack: task.pack,
                    root: task.root,
                    path: task.path.clone(),
                    task_id: task.id,
                    task_name: task.name.clone(),
                    field: reference.field.clone(),
                }))
                .collect();
            let mut roots: BTreeSet<(usize, usize)> = references.iter().map(|reference| (reference.pack, reference.root)).collect();
            roots.insert((pack, root));
            (references, duplicates, roots)
        };
        let mut report = TaskIdChange { old_id, new_id, name, references, duplicates, roots: roots.len(), state: None };
        if !apply {
            return Ok(report);
        }

        let replacements = HashMap::from([(old_id, new_id)]);
        let mut changes = Vec::new();
        for (change_pack, change_root) in roots {
            let own = change_pack == pack && change_root == root;
            let root_before = if own { before.clone() } else { self.current_root(change_pack, change_root)? };
            let mut node = decode_exact(&self.schema, &root_before, version)?;
            if own {
                let task = task_at_mut(&mut node, task_path)?;
                let id = task.child_mut("fixed").and_then(|fixed| fixed.child_mut("id")).ok_or("The quest has no ID field")?;
                let value = match id.value {
                    Value::I64(_) => Value::I64(i64::from(new_id)),
                    _ => Value::U64(u64::from(new_id)),
                };
                id.set_value(value)?;
            }
            if duplicates == 0 || own {
                rewrite_internal_task_references(&mut node, "", &replacements)?;
            }
            let mut after = node.encode()?;
            if own {
                // Parent, sibling and child links name the quest by ID.
                after = sync_hierarchy_links(&self.schema, version, &[&root_before], after)?;
            }
            verify_task_root(&self.schema, &after, version, "The ID change would make a task invalid")?;
            if after != root_before {
                changes.push(RootChange { pack: change_pack, root: change_root, before: root_before, after });
            }
        }
        self.apply_changes(&changes)?;
        // A freed ID is never handed to a clone, since references to it may survive elsewhere.
        self.id_floor = self.id_floor.max(old_id).max(new_id);
        self.journal.record(EntryDetails { label: "Change quest ID".into(), task_id: new_id, task_name: report.name.clone(), field: "fixed.id".into(), old: old_id.to_string(), new: new_id.to_string() }, changes);
        report.state = Some(self.edit_state());
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn changes_an_id_and_every_reference_then_undoes() {
        let path = r"E:/Games/ForsakenJD/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        let mut document = TaskDocument::open(path).unwrap();
        while !document.search("", 0).indexed {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // A top-level quest other quests refer to, with a unique ID.
        let (target, referrers) = {
            let index = document.search.read().unwrap();
            let mut counts: HashMap<u32, usize> = HashMap::new();
            for task in &index.entries {
                *counts.entry(task.id).or_default() += 1;
            }
            let referenced: HashMap<u32, BTreeSet<(usize, usize)>> = index.entries.iter().flat_map(|task| task.references.iter().filter(|reference| !reference.element).map(move |reference| (reference.target_id, (task.pack, task.root)))).fold(HashMap::new(), |mut map, (id, at)| { map.entry(id).or_default().insert(at); map });
            let target = index.entries.iter().find(|task| task.path.is_empty() && counts[&task.id] == 1 && referenced.get(&task.id).is_some_and(|roots| roots.iter().any(|at| *at != (task.pack, task.root)))).unwrap().clone();
            let referrers = referenced[&target.id].clone();
            (target, referrers)
        };
        let originals: Vec<(usize, usize, Vec<u8>)> = referrers.iter().chain(std::iter::once(&(target.pack, target.root))).map(|(pack, root)| (*pack, *root, document.current_root(*pack, *root).unwrap())).collect();
        let fresh = (1..).map(|offset| 900_000 + offset).find(|id| !document.search.read().unwrap().by_id.contains_key(id)).unwrap();

        assert!(document.change_task_id(target.pack, target.root, &[], target.id + 1, fresh, false).is_err(), "a stale expected ID is refused");
        let taken = document.search.read().unwrap().entries.iter().find(|task| task.id != target.id).unwrap().id;
        assert!(document.change_task_id(target.pack, target.root, &[], target.id, taken, false).unwrap_err().contains("already used"));
        let preview = document.change_task_id(target.pack, target.root, &[], target.id, fresh, false).unwrap();
        assert!(preview.references.len() >= referrers.len() && preview.state.is_none());
        assert_eq!(document.current_root(target.pack, target.root).unwrap(), originals.last().unwrap().2, "a preview changes nothing");

        document.change_task_id(target.pack, target.root, &[], target.id, fresh, true).unwrap();
        let index = document.search.read().unwrap();
        assert!(index.by_id.contains_key(&fresh) && !index.by_id.contains_key(&target.id));
        assert!(!index.entries.iter().any(|task| task.references.iter().any(|reference| !reference.element && reference.target_id == target.id)), "no reference to the old ID is left");
        assert!(index.entries.iter().filter(|task| task.references.iter().any(|reference| reference.target_id == fresh)).count() >= referrers.len());
        drop(index);
        document.undo().unwrap();
        for (pack, root, bytes) in originals {
            assert_eq!(document.current_root(pack, root).unwrap(), bytes);
    
    }
        // A subquest: its parent's and siblings' hierarchy links follow the new ID.
        let sub = {
            let index = document.search.read().unwrap();
            index.entries.iter().find(|task| task.path.len() == 1 && task.path[0] > 0 && index.entries.iter().filter(|other| other.id == task.id).count() == 1 && index.entries.iter().any(|other| other.links.is_some_and(|links| links.contains(&task.id)) && other.id != task.id)).unwrap().clone()
        };
        let before = document.current_root(sub.pack, sub.root).unwrap();
        document.change_task_id(sub.pack, sub.root, &sub.path, sub.id, fresh, true).unwrap();
        {
            let index = document.search.read().unwrap();
            assert!(!index.entries.iter().any(|task| task.links.is_some_and(|links| links.contains(&sub.id))), "no link names the old ID");
            assert!(index.entries.iter().any(|task| task.id != fresh && task.links.is_some_and(|links| links.contains(&fresh))));
        }
        document.undo().unwrap();
        assert_eq!(document.current_root(sub.pack, sub.root).unwrap(), before);
    }
}
