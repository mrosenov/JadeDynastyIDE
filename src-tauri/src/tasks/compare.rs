//! Compares the open task set with another \`tasks.data\` and copies tasks or
//! field values from it.
//!
//! Tasks pair by ID. Roots whose bytes are identical (same position in the
//! tree) are counted without decoding; the rest are decoded in parallel and
//! compared field by field (\`fixed.premise_tasks[0]\`). Copying builds
//! import rows and goes through the JSON import planner, so it follows the
//! inspector's rules and lands as one undo step. Field values copy across
//! versions when the path and binary type match; complete missing top-level
//! trees copy only between identical layouts.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as Json};

use super::browser::{display_value, editable_type, locked_field, read_nested_index, structural_paths, task_at, TaskDocument, TaskSearchEntry};
use super::container::TaskContainer;
use super::json::{json_value, schema_digest};
use super::schema::{decode_exact, FieldType, Node, Schema};

/// Changed and one-sided tasks listed in a report (the rest are only counted).
const LISTED: usize = 2000;

type Position = (usize, usize, Vec<usize>);

/// The other task set: opened read-only and indexed once.
pub struct ComparedTasks {
    pub path: String,
    pub version: u32,
    pub(crate) digest: String,
    pub(crate) container: TaskContainer,
    pub(crate) schema: Schema,
    pub(crate) entries: Vec<TaskSearchEntry>,
    pub(crate) size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareFile {
    pub path: String,
    pub version: u32,
    pub roots: usize,
    pub tasks: usize,
    pub packs: usize,
    pub size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiff {
    /// Dotted path, as in JSON exports.
    pub field: String,
    /// Value in the open file; none when the field does not exist there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub this: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    /// The compared value can be written into the open task.
    pub copyable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedTask {
    pub id: u32,
    pub name: String,
    /// The compared file's name when it differs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_name: Option<String>,
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    /// It has another parent in the compared file.
    pub moved: bool,
    /// Its direct subquests (by ID and order) differ.
    pub subtasks_differ: bool,
    /// Differing fields; their values are read with `task_compare_fields`.
    pub field_count: usize,
    pub copyable_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OneSidedTask {
    pub id: u32,
    pub name: String,
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    /// Tasks in its tree, itself included (top-level tasks).
    pub tasks: usize,
    /// Can be added to the open file as a whole tree (compared-only top-level tasks).
    pub copyable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCompareReport {
    pub this: CompareFile,
    pub other: CompareFile,
    /// Same task version and layout: whole missing trees can be copied.
    pub same_layout: bool,
    pub identical: usize,
    pub changed_count: usize,
    pub changed: Vec<ChangedTask>,
    pub only_this_count: usize,
    pub only_this: Vec<OneSidedTask>,
    pub only_other_count: usize,
    pub only_other: Vec<OneSidedTask>,
    /// IDs several tasks share on either side; they are not paired.
    pub ambiguous: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyField {
    pub id: u32,
    pub field: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopySelection {
    #[serde(default)]
    pub fields: Vec<CopyField>,
    /// Tasks whose every copyable differing field is copied.
    #[serde(default)]
    pub all_fields: Vec<u32>,
    /// Compared-only top-level tasks to add with their subquests.
    #[serde(default)]
    pub tasks: Vec<u32>,
}

/// Every pack's bytes, to slice roots from without reopening files.
pub(crate) fn read_packs(container: &TaskContainer) -> Result<Vec<Vec<u8>>, String> {
    container.packs.iter().map(|pack| std::fs::read(pack.path()).map_err(|error| format!("{}: {error}", pack.path().display()))).collect()
}

pub(crate) fn slice<'a>(container: &TaskContainer, packs: &'a [Vec<u8>], pack: usize, root: usize) -> Result<&'a [u8], String> {
    let range = container.packs.get(pack).ok_or("Task pack does not exist")?.root_range(root)?;
    packs.get(pack).and_then(|data| data.get(range.start as usize..range.end as usize)).ok_or_else(|| format!("Root {}:{} is outside its pack", pack + 1, root + 1))
}

/// Every editable value of one task (not its subtasks) with its type, by dotted path.
fn comparable_fields(schema: &Schema, task: &Node) -> BTreeMap<String, (String, FieldType)> {
    fn walk(node: &Node, path: &mut Vec<String>, locked: &HashSet<Vec<String>>, output: &mut BTreeMap<String, (String, FieldType)>) {
        if node.children().is_empty() {
            if editable_type(&node.ty) && locked_field(locked, path).is_none() {
                output.insert(super::json::dotted(path), (display_value(node).0.unwrap_or_default(), node.ty.clone()));
            }
            return;
        }
        for child in node.children() {
            path.push(child.name.clone());
            walk(child, path, locked, output);
            path.pop();
        }
    }
    let locked = structural_paths(schema, task);
    let mut output = BTreeMap::new();
    for field in task.children().iter().filter(|field| field.name != "subtasks") {
        walk(field, &mut vec![field.name.clone()], &locked, &mut output);
    }
    output
}

/// A value a field that one version lacks would hold anyway.
fn is_default(display: &str) -> bool {
    display.is_empty() || display == "0" || display == "false" || display.split(' ').all(|byte| byte == "00")
}

/// Field differences of one paired task. Fields only one side has count only
/// when they hold something; copying needs the same path and binary type.
fn diff_fields(this_schema: &Schema, this_task: &Node, other_schema: &Schema, other_task: &Node) -> Vec<FieldDiff> {
    let this_fields = comparable_fields(this_schema, this_task);
    let other_fields = comparable_fields(other_schema, other_task);
    let mut fields = Vec::new();
    for field in this_fields.keys().chain(other_fields.keys()).collect::<std::collections::BTreeSet<_>>() {
        let (mine, yours) = (this_fields.get(field), other_fields.get(field));
        match (mine, yours) {
            (Some(mine), Some(yours)) if mine.0 == yours.0 => continue,
            (Some(only), None) | (None, Some(only)) if is_default(&only.0) => continue,
            _ => {}
        }
        let copyable = matches!((mine, yours), (Some(mine), Some(yours)) if mine.1 == yours.1);
        fields.push(FieldDiff { field: field.clone(), this: mine.map(|value| value.0.clone()), other: yours.map(|value| value.0.clone()), copyable });
    }
    fields
}

fn child_ids(task: &Node) -> Vec<u32> {
    task.child("subtasks").map(Node::children).unwrap_or_default().iter()
        .filter_map(|child| super::browser::task_heading(child).ok().map(|(id, _)| id)).collect()
}

/// Each ID used exactly once, with its position; the count of IDs used more than once.
pub(crate) fn unique_positions(entries: &[TaskSearchEntry]) -> (HashMap<u32, &TaskSearchEntry>, HashSet<u32>) {
    let mut seen = HashMap::<u32, &TaskSearchEntry>::new();
    let mut duplicated = HashSet::new();
    for entry in entries {
        if seen.insert(entry.id, entry).is_some() {
            duplicated.insert(entry.id);
        }
    }
    for id in &duplicated {
        seen.remove(id);
    }
    (seen, duplicated)
}

fn parent_id(positions: &HashMap<Position, u32>, entry: &TaskSearchEntry) -> u32 {
    entry.path.split_last().map_or(0, |(_, parent)| positions.get(&(entry.pack, entry.root, parent.to_vec())).copied().unwrap_or(0))
}

impl ComparedTasks {
    /// Opens and indexes a task set, with the user layout accepted for its version if needed.
    pub fn open(path: &str, user_dir: &Path) -> Result<Self, String> {
        let source = super::analyze::source_version(path)?;
        let schema = if source.supported {
            super::schema_for_version(source.version)?
        } else {
            let layout = super::layout::load(user_dir, source.version)?.ok_or_else(|| format!("tasks.data v{} has no accepted user layout", source.version))?;
            if !layout.is_verified() {
                return Err(format!("The tasks.data v{} user layout must pass exact verification before it can be compared", source.version));
            }
            layout.validate()?
        };
        let container = TaskContainer::open(path)?;
        let entries = read_nested_index(&container, &schema)?;
        let size = std::fs::metadata(container.index_path()).map(|metadata| metadata.len()).unwrap_or(0) + container.total_pack_bytes();
        Ok(Self { path: container.index_path().display().to_string(), version: container.header.version, digest: schema_digest(&schema)?, container, schema, entries, size })
    }

    /// Import rows for a copy: the selected field values of compared tasks, and
    /// complete trees (\`_raw\`) of the selected compared-only top-level tasks.
    pub fn copy_rows(&self, selection: &CopySelection) -> Result<Vec<Json>, String> {
        let (positions, _) = unique_positions(&self.entries);
        let mut decoded = HashMap::<(usize, usize), Node>::new();
        let mut rows = BTreeMap::<u32, Map<String, Json>>::new();
        let decode = |pack: usize, root: usize, decoded: &mut HashMap<(usize, usize), Node>| -> Result<(), String> {
            if !decoded.contains_key(&(pack, root)) {
                decoded.insert((pack, root), decode_exact(&self.schema, &self.container.root(pack, root)?, self.version)?);
            }
            Ok(())
        };
        for choice in &selection.fields {
            let entry = positions.get(&choice.id).ok_or_else(|| format!("Task {} is not (uniquely) in the compared file", choice.id))?;
            decode(entry.pack, entry.root, &mut decoded)?;
            let task = task_at(&decoded[&(entry.pack, entry.root)], &entry.path)?;
            let node = super::browser::node_at(task, &super::json::field_path(&choice.field)?)?;
            let value = json_value(node).ok_or_else(|| format!("{} of task {} has no single value", choice.field, choice.id))?;
            let row = rows.entry(choice.id).or_default();
            row.entry("fields").or_insert_with(|| Json::Object(Map::new())).as_object_mut().unwrap().insert(choice.field.clone(), value);
        }
        for id in &selection.tasks {
            let entry = positions.get(id).ok_or_else(|| format!("Task {id} is not (uniquely) in the compared file"))?;
            if !entry.path.is_empty() {
                return Err(format!("Task {id} is a subquest; only top-level tasks can be copied whole"));
            }
            let row = rows.entry(*id).or_default();
            row.insert("_pack".into(), json!(entry.pack));
            row.insert("_raw".into(), json!(self.container.root(entry.pack, entry.root)?.iter().map(|byte| format!("{byte:02x}")).collect::<String>()));
        }
        Ok(rows.into_iter().map(|(id, mut row)| {
            row.insert("id".into(), json!(id));
            Json::Object(row)
        }).collect())
    }
}

impl TaskDocument {
    /// Same task version and binary layout as the compared set.
    pub fn same_layout(&self, other: &ComparedTasks) -> Result<bool, String> {
        Ok(other.version == self.container.header.version && other.digest == schema_digest(&self.schema)?)
    }

    /// The differing fields of task `id`, present once in both sets.
    pub fn compare_task_fields(&self, other: &ComparedTasks, id: u32) -> Result<Vec<FieldDiff>, String> {
        let this = {
            let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
            let mut matches = index.entries.iter().filter(|entry| entry.id == id);
            let found = matches.next().cloned().ok_or_else(|| format!("Task {id} is not in the open file"))?;
            if matches.next().is_some() {
                return Err(format!("Several open tasks use ID {id}"));
            }
            found
        };
        let (positions, _) = unique_positions(&other.entries);
        let theirs = positions.get(&id).ok_or_else(|| format!("Task {id} is not (uniquely) in the compared file"))?;
        let this_root = decode_exact(&self.schema, &self.current_root(this.pack, this.root)?, self.container.header.version)?;
        let other_root = decode_exact(&other.schema, &other.container.root(theirs.pack, theirs.root)?, other.version)?;
        Ok(diff_fields(&self.schema, task_at(&this_root, &this.path)?, &other.schema, task_at(&other_root, &theirs.path)?))
    }

    /// Expands `all_fields` into the copyable differing fields of those tasks.
    pub fn resolve_copy(&self, other: &ComparedTasks, selection: &CopySelection) -> Result<CopySelection, String> {
        let mut fields = selection.fields.clone();
        for id in &selection.all_fields {
            fields.extend(self.compare_task_fields(other, *id)?.into_iter().filter(|field| field.copyable).map(|field| CopyField { id: *id, field: field.field }));
        }
        Ok(CopySelection { fields, all_fields: Vec::new(), tasks: selection.tasks.clone() })
    }

    pub fn compare_with(&self, other: &ComparedTasks) -> Result<TaskCompareReport, String> {
        let started = Instant::now();
        let version = self.container.header.version;
        let this_entries = {
            let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
            if !index.indexed {
                return Err("Tasks are still being indexed. Compare again in a moment".into());
            }
            index.entries.clone()
        };
        let same_layout = self.same_layout(other)?;
        let (this_by_id, this_duplicates) = unique_positions(&this_entries);
        let (other_by_id, other_duplicates) = unique_positions(&other.entries);
        let this_positions = this_entries.iter().map(|entry| ((entry.pack, entry.root, entry.path.clone()), entry.id)).collect::<HashMap<Position, u32>>();
        let other_positions = other.entries.iter().map(|entry| ((entry.pack, entry.root, entry.path.clone()), entry.id)).collect::<HashMap<Position, u32>>();
        let this_ids = this_entries.iter().map(|entry| entry.id).collect::<HashSet<_>>();
        let other_ids = other.entries.iter().map(|entry| entry.id).collect::<HashSet<_>>();

        let this_packs = read_packs(&self.container)?;
        let other_packs = read_packs(&other.container)?;
        let this_root = |pack: usize, root: usize| -> Result<Vec<u8>, String> {
            if self.modified.contains_key(&(pack, root)) || root >= self.container.packs[pack].root_count() {
                self.current_root(pack, root)
            } else {
                slice(&self.container, &this_packs, pack, root).map(<[u8]>::to_vec)
            }
        };

        // Pair by ID; identical roots at the same position need no decoding.
        let mut identical = 0usize;
        let mut units = BTreeMap::<((usize, usize), (usize, usize)), Vec<(&TaskSearchEntry, &TaskSearchEntry)>>::new();
        let mut same_root = HashMap::<((usize, usize), (usize, usize)), bool>::new();
        for (id, this) in &this_by_id {
            let Some(theirs) = other_by_id.get(id) else { continue };
            let key = ((this.pack, this.root), (theirs.pack, theirs.root));
            let same = match same_root.get(&key) {
                Some(same) => *same,
                None => {
                    let same = this_root(this.pack, this.root)? == slice(&other.container, &other_packs, theirs.pack, theirs.root)?;
                    same_root.insert(key, same);
                    same
                }
            };
            if same && this.path == theirs.path {
                identical += 1;
            } else {
                units.entry(key).or_default().push((this, theirs));
            }
        }

        // Decode and compare the rest in parallel, one root pair per unit.
        let units = units.into_iter().map(|(key, pairs)| Ok((this_root(key.0 .0, key.0 .1)?, key.1, pairs))).collect::<Result<Vec<_>, String>>()?;
        let next = AtomicUsize::new(0);
        let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(units.len().max(1));
        let results = std::thread::scope(|scope| -> Result<Vec<(usize, Vec<ChangedTask>)>, String> {
            let mut handles = Vec::new();
            for _ in 0..workers {
                handles.push(scope.spawn(|| -> Result<Vec<(usize, Vec<ChangedTask>)>, String> {
                    let mut output = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some((this_bytes, (other_pack, other_root), pairs)) = units.get(index) else { break };
                        let this_node = decode_exact(&self.schema, this_bytes, version)?;
                        let other_node = decode_exact(&other.schema, slice(&other.container, &other_packs, *other_pack, *other_root)?, other.version)?;
                        let mut same = 0;
                        let mut changed = Vec::new();
                        for (this, theirs) in pairs {
                            let this_task = task_at(&this_node, &this.path)?;
                            let other_task = task_at(&other_node, &theirs.path)?;
                            let fields = diff_fields(&self.schema, this_task, &other.schema, other_task);
                            let moved = parent_id(&this_positions, this) != parent_id(&other_positions, theirs);
                            let subtasks_differ = child_ids(this_task) != child_ids(other_task);
                            if fields.is_empty() && !moved && !subtasks_differ {
                                same += 1;
                                continue;
                            }
                            changed.push(ChangedTask {
                                id: this.id,
                                name: this.name.clone(),
                                other_name: (theirs.name != this.name).then(|| theirs.name.clone()),
                                pack: this.pack,
                                root: this.root,
                                path: this.path.clone(),
                                moved,
                                subtasks_differ,
                                field_count: fields.len(),
                                copyable_count: fields.iter().filter(|field| field.copyable).count(),
                            });
                        }
                        output.push((same, changed));
                    }
                    Ok(output)
                }));
            }
            let mut all = Vec::new();
            for handle in handles {
                all.extend(handle.join().map_err(|_| "Compare worker panicked")??);
            }
            Ok(all)
        })?;
        let mut changed = Vec::new();
        for (same, tasks) in results {
            identical += same;
            changed.extend(tasks);
        }
        changed.sort_by(|left, right| (left.pack, left.root, &left.path).cmp(&(right.pack, right.root, &right.path)));
        let changed_count = changed.len();
        changed.truncate(LISTED);

        // One-sided tasks; compared-only top-level trees may be copied whole.
        let tree_size = |entries: &[TaskSearchEntry], pack: usize, root: usize| entries.iter().filter(|entry| entry.pack == pack && entry.root == root).count();
        let mut only_this = this_entries.iter().filter(|entry| !other_ids.contains(&entry.id)).collect::<Vec<_>>();
        let mut only_other = other.entries.iter().filter(|entry| !this_ids.contains(&entry.id)).collect::<Vec<_>>();
        only_this.sort_by(|left, right| (left.pack, left.root, &left.path).cmp(&(right.pack, right.root, &right.path)));
        only_other.sort_by(|left, right| (left.pack, left.root, &left.path).cmp(&(right.pack, right.root, &right.path)));
        let one_sided = |entry: &TaskSearchEntry, entries: &[TaskSearchEntry], copy: Option<(bool, Option<String>)>| OneSidedTask {
            id: entry.id,
            name: entry.name.clone(),
            pack: entry.pack,
            root: entry.root,
            path: entry.path.clone(),
            tasks: if entry.path.is_empty() { tree_size(entries, entry.pack, entry.root) } else { 1 },
            copyable: copy.as_ref().is_some_and(|(copyable, _)| *copyable),
            reason: copy.and_then(|(_, reason)| reason),
        };
        let copy_check = |entry: &TaskSearchEntry| -> (bool, Option<String>) {
            if !entry.path.is_empty() {
                return (false, Some("Subquests cannot be copied on their own; copy their top-level task".into()));
            }
            if !same_layout {
                return (false, Some(format!("Whole tasks copy only between identical layouts (open v{version}, compared v{})", other.version)));
            }
            match other.entries.iter().filter(|task| task.pack == entry.pack && task.root == entry.root).find(|task| this_ids.contains(&task.id)) {
                Some(clash) => (false, Some(format!("Its subquest ID {} is already used in the open file", clash.id))),
                None if other_duplicates.contains(&entry.id) => (false, Some("Its ID is used by several compared tasks".into())),
                None => (true, None),
            }
        };
        let only_this_count = only_this.len();
        let only_other_count = only_other.len();
        Ok(TaskCompareReport {
            this: CompareFile { path: self.summary.path.clone(), version, roots: self.summary.root_count, tasks: this_entries.len(), packs: self.container.packs.len(), size: self.summary.size },
            other: CompareFile { path: other.path.clone(), version: other.version, roots: other.container.header.root_count as usize, tasks: other.entries.len(), packs: other.container.packs.len(), size: other.size },
            same_layout,
            identical,
            changed_count,
            changed,
            only_this_count,
            only_this: only_this.into_iter().take(LISTED).map(|entry| one_sided(entry, &this_entries, None)).collect(),
            only_other_count,
            only_other: only_other.into_iter().take(LISTED).map(|entry| one_sided(entry, &other.entries, Some(copy_check(entry)))).collect(),
            ambiguous: this_duplicates.union(&other_duplicates).count(),
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::browser::FieldEdit;
    use crate::tasks::save::SaveOptions;

    const V165: &str = r"E:/Games/XtremeJade/element/data/tasks.data";

    fn indexed(path: &str) -> TaskDocument {
        let document = TaskDocument::open(path).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(120);
        while !document.search("", 0).indexed {
            assert!(Instant::now() < deadline, "background task index timed out");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        document
    }

    #[test]
    fn compares_edits_and_copies_them_back() {
        if !Path::new(V165).is_file() {
            return;
        }
        let compared = ComparedTasks::open(V165, Path::new(".")).unwrap();
        let mut document = indexed(V165);
        let same = document.compare_with(&compared).unwrap();
        assert!(same.same_layout);
        assert_eq!((same.changed_count, same.only_this_count, same.only_other_count), (0, 0, 0));
        assert!(same.identical > 27_000 && same.identical <= same.this.tasks);

        let parent = document.summary().roots.into_iter().find(|root| root.child_count > 0).unwrap();
        let original = document.current_root(parent.pack, parent.root).unwrap();
        document.edit_field(FieldEdit { pack: parent.pack, root: parent.root, task_path: Vec::new(), field_path: vec!["fixed".into(), "name".into()], value: "JD IDE compare test".into() }).unwrap();
        let clone = document.clone_subtask(parent.pack, parent.root, &[0]).unwrap();
        let report = document.compare_with(&compared).unwrap();
        let renamed = report.changed.iter().find(|task| task.id == parent.id).unwrap();
        assert!(renamed.field_count >= 1 && renamed.copyable_count >= 1);
        let fields = document.compare_task_fields(&compared, parent.id).unwrap();
        let name = fields.iter().find(|field| field.field == "fixed.name").unwrap();
        assert_eq!((name.this.as_deref(), name.other.as_deref(), name.copyable), (Some("JD IDE compare test"), Some(parent.name.as_str()), true));
        assert!(renamed.subtasks_differ, "the clone adds a subquest");
        assert!(report.only_this.iter().any(|task| task.id == clone.id));

        // Copy the name back and drop the clone: the root returns to its original bytes.
        let selection = document.resolve_copy(&compared, &CopySelection { fields: Vec::new(), all_fields: vec![parent.id], tasks: Vec::new() }).unwrap();
        assert!(selection.fields.iter().any(|field| field.field == "fixed.name"));
        let rows = compared.copy_rows(&selection).unwrap();
        let copied = document.copy_rows(&rows, true).unwrap();
        assert_eq!((copied.changing, copied.rejected), (1, 0));
        let preview = document.delete_subtask_preview(parent.pack, parent.root, &clone.path).unwrap();
        document.delete_subtask(parent.pack, parent.root, &clone.path, &preview.token, true).unwrap();
        assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), original);
        let report = document.compare_with(&compared).unwrap();
        assert_eq!((report.changed_count, report.only_this_count), (0, 0));
    }

    #[test]
    fn copies_a_missing_top_level_tree_from_a_saved_copy() {
        if !Path::new(V165).is_file() {
            return;
        }
        let folder = std::env::temp_dir().join(format!("jdide-task-compare-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let copy = folder.join("tasks.data");
        let mut source = indexed(V165);
        let parent = source.summary().roots.into_iter().find(|root| root.child_count > 0).unwrap();
        let clone = source.clone_root_task(parent.pack, parent.root).unwrap();
        source.save(&SaveOptions { path: copy.display().to_string(), backup: false }).unwrap();
        drop(source);

        let compared = ComparedTasks::open(&copy.display().to_string(), Path::new(".")).unwrap();
        let mut document = indexed(V165);
        let report = document.compare_with(&compared).unwrap();
        let missing = report.only_other.iter().find(|task| task.id == clone.id).unwrap();
        assert!(missing.copyable && missing.path.is_empty() && missing.tasks == clone.tasks);
        let rows = compared.copy_rows(&CopySelection { fields: Vec::new(), all_fields: Vec::new(), tasks: vec![clone.id] }).unwrap();
        let copied = document.copy_rows(&rows, report.same_layout).unwrap();
        assert_eq!((copied.adding, copied.rejected), (1, 0));
        let after = document.compare_with(&compared).unwrap();
        assert_eq!((after.changed_count, after.only_this_count, after.only_other_count), (0, 0, 0));
        document.undo().unwrap();
        drop(compared);
        let _ = std::fs::remove_dir_all(&folder);
    }
}


