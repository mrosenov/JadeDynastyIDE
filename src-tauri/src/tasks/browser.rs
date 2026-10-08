//! Read-only view models for the tasks.data browser.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::{client::Resources, elements::Document};

use super::container::{Pack, TaskContainer, ROOTS_PER_PACK};
use super::edit::{ChangedRoot, EditState, EntryDetails, HistoryEntry, Journal, RootChange};
use super::schema::{decode_exact, probe_root_integer_validated, probe_task_index_validated, FieldType, Node, ProbedTaskReference, Schema, Value};
use super::schema_for_version;

const ROOT_HEADING_BYTES: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootSummary {
    pub index: usize,
    pub pack: usize,
    pub root: usize,
    pub id: u32,
    pub name: String,
    pub child_count: usize,
    pub byte_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub path: String,
    pub version: u32,
    pub export_version: u32,
    pub root_count: usize,
    pub pack_count: usize,
    pub size: u64,
    pub roots: Vec<RootSummary>,
    pub user_layout: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSearchEntry {
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub child_count: usize,
    #[serde(skip)]
    pub(crate) references: Vec<ProbedTaskReference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSearchReport {
    pub indexed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub total: usize,
    pub matches: Vec<TaskSearchEntry>,
}

#[derive(Default)]
struct TaskSearchIndex {
    indexed: bool,
    error: Option<String>,
    entries: Vec<TaskSearchEntry>,
    by_id: HashMap<u32, TaskSearchEntry>,
    edited_roots: HashSet<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub id: u32,
    pub name: String,
    pub path: Vec<usize>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpretation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FieldView>,
    pub raw: bool,
    pub path: Vec<String>,
    pub editable: bool,
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<FieldReference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldReference {
    pub kind: String,
    pub id: u32,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub root_bytes: usize,
    pub task_offset: usize,
    pub task_size: usize,
    pub tree: TreeNode,
    pub fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldEdit {
    pub pack: usize,
    pub root: usize,
    pub task_path: Vec<usize>,
    pub field_path: Vec<String>,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCloneReport {
    pub state: EditState,
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub tasks: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMoveReport {
    pub state: EditState,
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub tasks: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeleteReference {
    pub source_id: u32,
    pub source_name: String,
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub field: String,
    pub target_id: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeletePreview {
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub tasks: usize,
    pub reference_count: usize,
    pub references_truncated: bool,
    pub references: Vec<TaskDeleteReference>,
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDeleteReport {
    pub state: EditState,
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub tasks: usize,
}

impl TaskDetail {
    pub fn resolve_references(&mut self, document: Option<&Document>, resources: Option<&Resources>) {
        fn walk(fields: &mut [FieldView], document: Option<&Document>, resources: Option<&Resources>) {
            for field in fields {
                if let Some(reference) = field.reference.as_mut() {
                    match reference.kind.as_str() {
                        "element" => {
                            if let Some((list, row, label)) = document.and_then(|doc| doc.resolve_essence_id(reference.id)) {
                                reference.list = Some(list);
                                reference.row = Some(row);
                                reference.label = label;
                            } else {
                                reference.label = "Element record not found".into();
                            }
                        }
                        "skill" => {
                            reference.label = resources.and_then(|value| value.skill_name(reference.id)).unwrap_or_else(|| "Skill not found".into());
                            reference.description = resources.and_then(|value| value.skill_description(reference.id));
                        }
                        "buff" => {
                            reference.label = resources.and_then(|value| value.buff_name(reference.id)).unwrap_or_else(|| "Buff not found".into());
                            reference.description = resources.and_then(|value| value.buff_description(reference.id));
                        }
                        "title" => {
                            reference.label = resources.and_then(|value| value.title_name(reference.id)).unwrap_or_else(|| "Title not found".into());
                            reference.description = resources.and_then(|value| value.title_description(reference.id));
                        }
                        _ => {}
                    }
                }
                walk(&mut field.children, document, resources);
            }
        }
        walk(&mut self.fields, document, resources);
    }
}

pub struct TaskDocument {
    pub(crate) container: TaskContainer,
    pub(crate) schema: Schema,
    pub(crate) summary: FileSummary,
    search: Arc<RwLock<TaskSearchIndex>>,
    pub(crate) cache: Option<CachedRoot>,
    pub(crate) modified: HashMap<(usize, usize), ModifiedRoot>,
    pub(crate) added_roots: HashMap<usize, Vec<Vec<u8>>>,
    pub(crate) journal: Journal,
    pub(crate) disk: HashMap<std::path::PathBuf, super::save::DiskStamp>,
    pub(crate) backed_up: HashSet<std::path::PathBuf>,
    /// The highest task ID handed out or deleted in this session. Fresh IDs
    /// start above it, so an ID freed by a delete or an undone clone is never
    /// given to a different quest while references to it may survive.
    id_floor: u32,
}

pub(crate) struct CachedRoot {
    pack: usize,
    root: usize,
    bytes: usize,
    pub(crate) node: Node,
    pub(crate) original: Node,
}

pub(crate) struct ModifiedRoot {
    pub(crate) original: Vec<u8>,
    pub(crate) current: Vec<u8>,
}

impl TaskDocument {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let container = TaskContainer::open(path)?;
        let schema = schema_for_version(container.header.version)?;
        Self::from_container(container, schema, false)
    }

    pub fn open_with_schema(path: impl AsRef<Path>, schema: Schema) -> Result<Self, String> {
        let container = TaskContainer::open(path)?;
        Self::from_container(container, schema, true)
    }

    fn from_container(container: TaskContainer, schema: Schema, user_layout: bool) -> Result<Self, String> {
        let roots = read_root_summaries(&container, &schema)?;
        let entries = roots.iter().map(|root| TaskSearchEntry {
                pack: root.pack,
                root: root.root,
                path: Vec::new(),
                id: root.id,
                name: root.name.clone(),
                child_count: root.child_count,
                references: Vec::new(),
            }).collect::<Vec<_>>();
        let by_id = entries.iter().map(|task| (task.id, task.clone())).collect();
        let search = Arc::new(RwLock::new(TaskSearchIndex { entries, by_id, ..TaskSearchIndex::default() }));
        let summary = FileSummary {
            path: container.index_path().display().to_string(),
            version: container.header.version,
            export_version: container.header.export_version,
            root_count: roots.len(),
            pack_count: container.packs.len(),
            size: std::fs::metadata(container.index_path()).map(|metadata| metadata.len()).unwrap_or(0)
                + container.total_pack_bytes(),
            roots,
            user_layout,
        };
        let background_search = search.clone();
        let background_container = container.clone();
        let background_schema = schema.clone();
        std::thread::spawn(move || {
            let result = read_nested_index(&background_container, &background_schema);
            let Ok(mut index) = background_search.write() else { return };
            match result {
                Ok(subtasks) => {
                    let edited_roots = index.edited_roots.clone();
                    index.entries.retain(|task| edited_roots.contains(&(task.pack, task.root)));
                    for task in subtasks {
                        if index.edited_roots.contains(&(task.pack, task.root)) {
                            continue;
                        }
                        index.entries.push(task);
                    }
                    index.by_id.clear();
                    for task in index.entries.clone() {
                        index.by_id.insert(task.id, task);
                    }
                    index.indexed = true;
                }
                Err(error) => index.error = Some(error),
            }
        });
        let disk = super::save::stamps(&container);
        Ok(Self { container, schema, summary, search, cache: None, modified: HashMap::new(), added_roots: HashMap::new(), journal: Journal::default(), disk, backed_up: HashSet::new(), id_floor: 0 })
    }

    pub fn summary(&self) -> FileSummary {
        self.summary.clone()
    }

    pub fn search(&self, query: &str, limit: usize) -> TaskSearchReport {
        let Ok(index) = self.search.read() else {
            return TaskSearchReport { indexed: false, error: Some("Task search index lock poisoned".into()), total: 0, matches: Vec::new() };
        };
        let query = query.trim().to_lowercase();
        let numeric = query.parse::<u32>().ok();
        let mut matches = index.entries.iter().filter(|task| {
            if let Some(id) = numeric {
                // Exact IDs sort first; ID prefixes and names such as "Level 30" follow.
                task.id == id || task.id.to_string().starts_with(&query) || task.name.to_lowercase().contains(&query)
            } else {
                task.name.to_lowercase().contains(&query) || task.id.to_string().contains(&query)
            }
        }).cloned().collect::<Vec<_>>();
        matches.sort_by_key(|task| (numeric != Some(task.id), !task.name.to_lowercase().starts_with(&query), task.path.len(), task.id));
        let total = matches.len();
        matches.truncate(limit);
        TaskSearchReport { indexed: index.indexed, error: index.error.clone(), total, matches }
    }

    pub fn task(&mut self, pack: usize, root: usize, path: &[usize]) -> Result<TaskDetail, String> {
        let reload = self.cache.as_ref().map_or(true, |cached| cached.pack != pack || cached.root != root);
        if reload {
            let bytes = self.current_root(pack, root)?;
            let node = decode_exact(&self.schema, &bytes, self.container.header.version)?;
            let original_bytes = if self.is_added_root(pack, root)? {
                bytes.clone()
            } else {
                self.modified.get(&(pack, root)).map(|root| root.original.clone()).unwrap_or_else(|| bytes.clone())
            };
            let original = decode_exact(&self.schema, &original_bytes, self.container.header.version)?;
            self.cache = Some(CachedRoot { pack, root, bytes: bytes.len(), node, original });
        }
        let cached = self.cache.as_ref().expect("cache was populated");
        let tree = tree_view(&cached.node, Vec::new())?;
        let selected = task_at(&cached.node, path)?;
        let original_selected = task_at(&cached.original, path)?;
        let (id, name) = task_heading(selected)?;
        let locked = structural_paths(&self.schema, selected);
        let fields = selected
            .children()
            .iter()
            .filter(|field| field.name != "subtasks")
            .map(|field| {
                let field_path = vec![field.name.clone()];
                let original = node_at(original_selected, &field_path).ok();
                self.field_view(field, original, &field.name, field_path, &locked)
            })
            .collect();
        Ok(TaskDetail {
            pack,
            root,
            path: path.to_vec(),
            id,
            name,
            root_bytes: cached.bytes,
            task_offset: selected.offset,
            task_size: selected.byte_len,
            tree,
            fields,
        })
    }

    fn field_view(&self, node: &Node, original: Option<&Node>, semantic: &str, path: Vec<String>, locked: &HashSet<Vec<String>>) -> FieldView {
        let children = node.children().iter().map(|child| {
            let child_semantic = if child.name.starts_with('[') { semantic } else { &child.name };
            let mut child_path = path.clone();
            child_path.push(child.name.clone());
            let original_child = original.and_then(|candidate| candidate.child(&child.name));
            self.field_view(child, original_child, child_semantic, child_path, locked)
        }).collect::<Vec<_>>();
        let raw = matches!(node.ty, FieldType::Raw { .. });
        let (value, interpretation) = display_value(node);
        let editable = children.is_empty()
            && !locked.contains(&path)
            && !(path.len() == 2 && path[0] == "fixed" && path[1] == "id")
            && matches!(node.ty,
                FieldType::I8 | FieldType::U8 | FieldType::Bool8 | FieldType::I16 | FieldType::U16 |
                FieldType::I32 | FieldType::U32 | FieldType::I64 | FieldType::U64 | FieldType::F32 |
                FieldType::F64 | FieldType::FixedUtf16 { .. } | FieldType::PrefixedUtf16 { .. } |
                FieldType::CountedUtf16 { .. } | FieldType::Raw { .. }
            );
        let changed = original.map_or(false, |candidate| candidate.encode().ok() != node.encode().ok());
        FieldView {
            name: node.name.clone(),
            offset: node.offset,
            size: node.byte_len,
            ty: type_name(&node.ty),
            value,
            interpretation,
            children,
            raw,
            path,
            editable,
            changed,
            reference: self.field_reference(semantic, node),
        }
    }

    fn field_reference(&self, semantic: &str, node: &Node) -> Option<FieldReference> {
        let id = match node.value {
            Value::U64(value) => u32::try_from(value).ok()?,
            Value::I64(value) => u32::try_from(value).ok()?,
            _ => return None,
        };
        if id == 0 {
            return None;
        }
        let semantic = semantic.to_ascii_lowercase();
        if matches!(semantic.as_str(), "task_id" | "new_task_id" | "terminate_task_ids") {
            let (target, indexed) = self.search.read().ok().map_or((None, false), |index| (index.by_id.get(&id).cloned(), index.indexed));
            return Some(FieldReference {
                kind: "task".into(),
                id,
                label: target.as_ref().map(|task| task.name.clone()).unwrap_or_else(|| if indexed { "Task not found" } else { "Indexing quests…" }.into()),
                description: None,
                list: None,
                row: None,
                pack: target.as_ref().map(|task| task.pack),
                root: target.as_ref().map(|task| task.root),
                path: target.map(|task| task.path).unwrap_or_default(),
            });
        }
        let kind = if matches!(semantic.as_str(), "item_id" | "drop_item_id" | "travel_item_id" | "replacement_item_id" | "monster_id" | "object_id") {
            "element"
        } else if semantic == "skill_id" {
            "skill"
        } else if semantic == "buff_id" {
            "buff"
        } else if semantic == "title_id" {
            "title"
        } else {
            return None;
        };
        Some(FieldReference {
            kind: kind.into(),
            id,
            label: format!("{kind} {id}"),
            description: None,
            list: None,
            row: None,
            pack: None,
            root: None,
            path: Vec::new(),
        })
    }

    pub fn edit_state(&self) -> EditState {
        self.journal.state(self.changed_root_keys().into_iter().map(|(pack, root)| ChangedRoot { pack, root }).collect())
    }

    pub fn history(&self) -> Vec<HistoryEntry> {
        let mut entries = self.journal.history();
        for entry in entries.iter_mut().filter(|entry| !entry.undone && entry.reverted_at.is_none()) {
            if let Some((_, changes)) = self.journal.revertable(entry.id) {
                entry.revert_blocked = self.revert_blocked(entry.id, &changes);
            }
        }
        entries
    }

    /// Takes back one applied entry of the history without undoing later ones.
    /// Allowed only while every task root it changed still holds its result;
    /// the entry then shows as reverted and Undo takes the revert back.
    pub fn revert_entry(&mut self, id: u64) -> Result<EditState, String> {
        if self.journal.is_reverted(id) {
            return Ok(self.edit_state());
        }
        let (label, changes) = self.journal.revertable(id).ok_or("That edit is not applied (undone, or no longer in the history)")?;
        if let Some(reason) = self.revert_blocked(id, &changes) {
            return Err(reason);
        }
        let inverse = changes.iter().rev().map(|change| RootChange {
            pack: change.pack,
            root: change.root,
            before: change.after.clone(),
            after: change.before.clone(),
        }).collect::<Vec<_>>();
        self.apply_changes(&inverse)?;
        self.journal.record_revert(EntryDetails {
            label: format!("Revert “{label}”"),
            task_id: 0,
            task_name: String::new(),
            field: String::new(),
            old: String::new(),
            new: String::new(),
        }, inverse, id);
        Ok(self.edit_state())
    }

    /// Why entry `id` cannot be reverted on its own, if it cannot.
    fn revert_blocked(&self, id: u64, changes: &[RootChange]) -> Option<String> {
        for change in changes {
            let current = self.current_root_optional(change.pack, change.root).ok().flatten();
            let unchanged = if change.after.is_empty() { current.is_none() } else { current.as_deref() == Some(change.after.as_slice()) };
            if !unchanged {
                return Some(match self.journal.later_change(id, change.pack, change.root) {
                    Some(label) => format!("“{label}” changed the same task root later. Undo or revert it first."),
                    None => "The task root changed after this edit.".into(),
                });
            }
            // A cloned top-level task can only be removed while it is the last appended one.
            if change.before.is_empty() && !change.after.is_empty() {
                let last = self.root_count(change.pack).ok()?.checked_sub(1);
                if last != Some(change.root) {
                    return Some("A top-level task cloned later into the same pack must be removed first.".into());
                }
            }
        }
        None
    }

    pub fn edit_field(&mut self, edit: FieldEdit) -> Result<EditState, String> {
        let before = self.current_root(edit.pack, edit.root)?;
        let mut decoded = decode_exact(&self.schema, &before, self.container.header.version)?;
        let selected = task_at_mut(&mut decoded, &edit.task_path)?;
        let locked = structural_paths(&self.schema, selected);
        if locked.contains(&edit.field_path) {
            return Err("This value controls the binary structure and cannot be edited directly".into());
        }
        if edit.field_path.len() == 2 && edit.field_path[0] == "fixed" && edit.field_path[1] == "id" {
            return Err("Task IDs are locked until task-reference rewriting is implemented".into());
        }
        let target = node_at(selected, &edit.field_path)?;
        if !target.children().is_empty() {
            return Err(format!("{} is a group; edit one of its values", target.name));
        }
        let ty = target.ty.clone();
        if !matches!(ty,
            FieldType::I8 | FieldType::U8 | FieldType::Bool8 | FieldType::I16 | FieldType::U16 |
            FieldType::I32 | FieldType::U32 | FieldType::I64 | FieldType::U64 | FieldType::F32 |
            FieldType::F64 | FieldType::FixedUtf16 { .. } | FieldType::PrefixedUtf16 { .. } |
            FieldType::CountedUtf16 { .. } | FieldType::Raw { .. }
        ) {
            return Err(format!("{} is not editable in this milestone", target.name));
        }
        let old = display_value(target).0.unwrap_or_default();
        let value = parse_value(&ty, &edit.value)?;
        let counted_text_length = match (&ty, &value) {
            (FieldType::CountedUtf16 { unit, .. }, Value::Text(text)) => {
                let units = text.encode_utf16().count();
                Some(match unit {
                    super::schema::TextLengthUnit::Utf16Units => units,
                    super::schema::TextLengthUnit::Bytes => units.checked_mul(2).ok_or("Text byte length overflow")?,
                })
            }
            _ => None,
        };
        let semantic = edit.field_path.iter().rev().find(|part| !part.starts_with('[')).map(String::as_str).unwrap_or_default();
        if matches!(semantic, "task_id" | "new_task_id" | "terminate_task_ids") {
            let id = match &value {
                Value::U64(value) => u32::try_from(*value).ok(),
                Value::I64(value) => u32::try_from(*value).ok(),
                _ => None,
            }.ok_or("Task references must contain a 32-bit task ID")?;
            if id != 0 {
                let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
                if index.indexed && !index.by_id.contains_key(&id) {
                    return Err(format!("Task ID {id} does not exist in the open task set"));
                }
            }
        }
        node_at_mut(selected, &edit.field_path)?.set_value(value)?;
        if let (FieldType::CountedUtf16 { count_field, .. }, Some(count)) = (&ty, counted_text_length) {
            let parent_path = &edit.field_path[..edit.field_path.len().saturating_sub(1)];
            let parent = node_at_mut(selected, parent_path)?;
            set_count(parent, count_field, count)?;
        }
        let after = decoded.encode()?;
        let verified = decode_exact(&self.schema, &after, self.container.header.version)
            .map_err(|error| format!("The edit would make this task invalid: {error}"))?;
        if verified.encode()? != after {
            return Err("The edited task did not pass an exact byte round trip".into());
        }
        if after == before {
            return Ok(self.edit_state());
        }
        let changed_task = task_at(&verified, &edit.task_path)?;
        let (task_id, task_name) = task_heading(changed_task)?;
        let new = display_value(node_at(changed_task, &edit.field_path)?).0.unwrap_or_default();
        let field = edit.field_path.iter().map(|part| label_path_part(part)).collect::<Vec<_>>().join(" › ");
        self.apply_changes(&[RootChange { pack: edit.pack, root: edit.root, before: before.clone(), after: after.clone() }])?;
        self.journal.record(EntryDetails {
            label: format!("Edit {field}"),
            task_id,
            task_name,
            field,
            old,
            new,
        }, vec![RootChange { pack: edit.pack, root: edit.root, before, after }]);
        Ok(self.edit_state())
    }

    pub fn clone_subtask(&mut self, pack: usize, root: usize, path: &[usize]) -> Result<TaskCloneReport, String> {
        let (&source_index, parent_path) = path.split_last().ok_or("Root-task cloning requires pack rebuilding and is not available yet")?;
        let before = self.current_root(pack, root)?;
        let mut decoded = decode_exact(&self.schema, &before, self.container.header.version)?;
        let source = task_at(&decoded, path)?.clone();
        let (source_id, source_name) = task_heading(&source)?;

        let mut source_ids = Vec::new();
        collect_task_ids(&source, &mut source_ids)?;
        let unique = source_ids.iter().copied().collect::<HashSet<_>>();
        if unique.len() != source_ids.len() {
            return Err("The selected subtree contains duplicate task IDs and cannot be cloned safely".into());
        }
        let (first, last) = self.fresh_task_ids(source_ids.len())?;
        let replacements = source_ids.into_iter().enumerate().map(|(index, old)| (old, (first + index as u64) as u32)).collect::<HashMap<_, _>>();
        let mut cloned = source;
        replace_own_task_ids(&mut cloned, &replacements)?;
        rewrite_internal_task_references(&mut cloned, "", &replacements)?;
        let (id, name) = task_heading(&cloned)?;

        let insert_at = source_index.checked_add(1).ok_or("Subtask position overflow")?;
        let parent = task_at_mut(&mut decoded, parent_path)?;
        let subtasks = parent.child_mut("subtasks").ok_or("The parent task has no subtask array")?;
        let count_field = match &subtasks.ty {
            FieldType::RecursiveArray { count_field, .. } => count_field.clone(),
            _ => return Err("The parent task's subtask field has an unexpected type".into()),
        };
        let count = {
            let children = subtasks.array_mut().ok_or("The parent task's subtask field is not an array")?;
            if insert_at > children.len() {
                return Err("The selected subtask position no longer exists".into());
            }
            children.insert(insert_at, cloned);
            children.len()
        };
        set_count(parent, &count_field, count)?;

        let after = sync_hierarchy_links(&self.schema, self.container.header.version, &[&before], decoded.encode()?)?;
        let verified = decode_exact(&self.schema, &after, self.container.header.version)
            .map_err(|error| format!("The cloned subtree would make this task invalid: {error}"))?;
        if verified.encode()? != after {
            return Err("The cloned subtree did not pass an exact byte round trip".into());
        }
        let mut cloned_path = parent_path.to_vec();
        cloned_path.push(insert_at);
        let tasks = replacements.len();
        let change = RootChange { pack, root, before: before.clone(), after: after.clone() };
        self.apply_changes(std::slice::from_ref(&change))?;
        self.journal.record(EntryDetails {
            label: if tasks == 1 { "Clone subquest".into() } else { format!("Clone subtree ({tasks} quests)") },
            task_id: id,
            task_name: name.clone(),
            field: "Task hierarchy".into(),
            old: format!("{source_id} · {source_name}"),
            new: format!("{id} · {name}"),
        }, vec![change]);
        self.id_floor = self.id_floor.max(last as u32);
        Ok(TaskCloneReport { state: self.edit_state(), pack, root, path: cloned_path, id, name, tasks })
    }

    pub fn clone_root_task(&mut self, pack: usize, root: usize) -> Result<TaskCloneReport, String> {
        let source = self.current_root(pack, root)?;
        let decoded = decode_exact(&self.schema, &source, self.container.header.version)?;
        let (source_id, source_name) = task_heading(&decoded)?;
        let mut source_ids = Vec::new();
        collect_task_ids(&decoded, &mut source_ids)?;
        let unique = source_ids.iter().copied().collect::<HashSet<_>>();
        if unique.len() != source_ids.len() {
            return Err("The selected task contains duplicate task IDs and cannot be cloned safely".into());
        }
        let (first, last) = self.fresh_task_ids(source_ids.len())?;
        let target_pack = if self.root_count(pack)? < ROOTS_PER_PACK {
            pack
        } else {
            (0..self.container.packs.len())
                .find(|candidate| self.root_count(*candidate).is_ok_and(|count| count < ROOTS_PER_PACK))
                .ok_or_else(|| format!("Every task pack already contains {ROOTS_PER_PACK} top-level tasks; creating another pack is not available yet"))?
        };
        let target_root = self.root_count(target_pack)?;
        let replacements = source_ids.into_iter().enumerate().map(|(index, old)| (old, (first + index as u64) as u32)).collect::<HashMap<_, _>>();
        let mut cloned = decoded;
        replace_own_task_ids(&mut cloned, &replacements)?;
        rewrite_internal_task_references(&mut cloned, "", &replacements)?;
        let (id, name) = task_heading(&cloned)?;
        let after = sync_hierarchy_links(&self.schema, self.container.header.version, &[&source], cloned.encode()?)?;
        verify_task_root(&self.schema, &after, self.container.header.version, "The cloned task would be invalid")?;
        let change = RootChange { pack: target_pack, root: target_root, before: Vec::new(), after };
        self.apply_changes(std::slice::from_ref(&change))?;
        let tasks = replacements.len();
        self.journal.record(EntryDetails {
            label: if tasks == 1 { "Clone task".into() } else { format!("Clone task tree ({tasks} quests)") },
            task_id: id,
            task_name: name.clone(),
            field: "Task hierarchy".into(),
            old: format!("{source_id} · {source_name}"),
            new: format!("{id} · {name}"),
        }, vec![change]);
        self.id_floor = self.id_floor.max(last as u32);
        Ok(TaskCloneReport { state: self.edit_state(), pack: target_pack, root: target_root, path: Vec::new(), id, name, tasks })
    }

    pub fn move_subtask(
        &mut self,
        source_pack: usize,
        source_root: usize,
        source_path: &[usize],
        destination_pack: usize,
        destination_root: usize,
        destination_path: &[usize],
    ) -> Result<TaskMoveReport, String> {
        if source_path.is_empty() {
            return Err("Root-task moving requires pack rebuilding and is not available yet".into());
        }
        if source_pack == destination_pack && source_root == destination_root && destination_path.starts_with(source_path) {
            return Err("A subquest cannot be moved into itself or one of its descendants".into());
        }
        let source_before = self.current_root(source_pack, source_root)?;
        let destination_before = if source_pack == destination_pack && source_root == destination_root {
            source_before.clone()
        } else {
            self.current_root(destination_pack, destination_root)?
        };
        let mut source_decoded = decode_exact(&self.schema, &source_before, self.container.header.version)?;
        let (id, name) = task_heading(task_at(&source_decoded, source_path)?)?;
        let mut ids = Vec::new();
        collect_task_ids(task_at(&source_decoded, source_path)?, &mut ids)?;
        let tasks = ids.len();

        let (destination_id, destination_name) = if source_pack == destination_pack && source_root == destination_root {
            task_heading(task_at(&source_decoded, destination_path)?)?
        } else {
            let decoded = decode_exact(&self.schema, &destination_before, self.container.header.version)?;
            task_heading(task_at(&decoded, destination_path)?)?
        };

        let (changes, selected_path) = if source_pack == destination_pack && source_root == destination_root {
            let adjusted_destination = task_path_after_removal(destination_path, source_path);
            let moved = remove_subtask(&mut source_decoded, source_path)?;
            let insertion = append_subtask(task_at_mut(&mut source_decoded, &adjusted_destination)?, moved)?;
            let after = sync_hierarchy_links(&self.schema, self.container.header.version, &[&source_before], source_decoded.encode()?)?;
            verify_task_root(&self.schema, &after, self.container.header.version, "Moving this subtree would make the task invalid")?;
            let mut path = adjusted_destination;
            path.push(insertion);
            (vec![RootChange { pack: source_pack, root: source_root, before: source_before, after }], path)
        } else {
            let moved = remove_subtask(&mut source_decoded, source_path)?;
            let source_after = sync_hierarchy_links(&self.schema, self.container.header.version, &[&source_before], source_decoded.encode()?)?;
            verify_task_root(&self.schema, &source_after, self.container.header.version, "Moving this subtree would make the source task invalid")?;

            let mut destination_decoded = decode_exact(&self.schema, &destination_before, self.container.header.version)?;
            let insertion = append_subtask(task_at_mut(&mut destination_decoded, destination_path)?, moved)?;
            let destination_after = sync_hierarchy_links(&self.schema, self.container.header.version, &[&source_before, &destination_before], destination_decoded.encode()?)?;
            verify_task_root(&self.schema, &destination_after, self.container.header.version, "Moving this subtree would make the destination task invalid")?;
            let mut path = destination_path.to_vec();
            path.push(insertion);
            (
                vec![
                    RootChange { pack: source_pack, root: source_root, before: source_before, after: source_after },
                    RootChange { pack: destination_pack, root: destination_root, before: destination_before, after: destination_after },
                ],
                path,
            )
        };
        self.apply_changes(&changes)?;
        self.journal.record(EntryDetails {
            label: if tasks == 1 { "Move subquest".into() } else { format!("Move subtree ({tasks} quests)") },
            task_id: id,
            task_name: name.clone(),
            field: "Task hierarchy".into(),
            old: format!("Parent {} · {}", source_root + 1, display_path(&source_path[..source_path.len() - 1])),
            new: format!("{} · {}", destination_id, destination_name),
        }, changes);
        Ok(TaskMoveReport { state: self.edit_state(), pack: destination_pack, root: destination_root, path: selected_path, id, name, tasks })
    }

    pub fn delete_subtask_preview(&self, pack: usize, root: usize, path: &[usize]) -> Result<TaskDeletePreview, String> {
        if path.is_empty() {
            return Err("Root-task deletion requires pack rebuilding and is not available yet".into());
        }
        let before = self.current_root(pack, root)?;
        let decoded = decode_exact(&self.schema, &before, self.container.header.version)?;
        let selected = task_at(&decoded, path)?;
        let (id, name) = task_heading(selected)?;
        let mut deleted_ids = Vec::new();
        collect_task_ids(selected, &mut deleted_ids)?;
        let deleted_ids = deleted_ids.into_iter().collect::<HashSet<_>>();
        let mut references = Vec::new();
        let mut reference_count = 0usize;
        let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
        if let Some(error) = &index.error {
            return Err(format!("Task references could not be indexed: {error}"));
        }
        if !index.indexed {
            return Err("Task references are still being indexed. Try deleting again in a moment".into());
        }
        for task in &index.entries {
            if task.pack == pack && task.root == root && task.path.starts_with(path) {
                continue;
            }
            for reference in task.references.iter().filter(|reference| deleted_ids.contains(&reference.target_id)) {
                reference_count += 1;
                if references.len() < 100 {
                    references.push(TaskDeleteReference {
                        source_id: task.id,
                        source_name: task.name.clone(),
                        pack: task.pack,
                        root: task.root,
                        path: task.path.clone(),
                        field: reference.field.split('.').map(label_path_part).collect::<Vec<_>>().join(" › "),
                        target_id: reference.target_id,
                    });
                }
            }
        }
        let token = delete_token(&before, path);
        Ok(TaskDeletePreview {
            pack,
            root,
            path: path.to_vec(),
            id,
            name,
            tasks: deleted_ids.len(),
            reference_count,
            references_truncated: reference_count > references.len(),
            references,
            token,
        })
    }

    pub fn delete_subtask(&mut self, pack: usize, root: usize, path: &[usize], token: &str, allow_referenced: bool) -> Result<TaskDeleteReport, String> {
        let preview = self.delete_subtask_preview(pack, root, path)?;
        if preview.token != token {
            return Err("The selected subtree changed after the confirmation was opened. Review the deletion again".into());
        }
        if preview.reference_count > 0 && !allow_referenced {
            return Err(format!("{} surviving task reference(s) point into this subtree", preview.reference_count));
        }
        let (&source_index, parent_path) = path.split_last().ok_or("Root-task deletion is not available yet")?;
        let before = self.current_root(pack, root)?;
        let mut decoded = decode_exact(&self.schema, &before, self.container.header.version)?;
        let decoded_before = decoded.clone();
        let parent = task_at_mut(&mut decoded, parent_path)?;
        let subtasks = parent.child_mut("subtasks").ok_or("The parent task has no subtask array")?;
        let count_field = match &subtasks.ty {
            FieldType::RecursiveArray { count_field, .. } => count_field.clone(),
            _ => return Err("The parent task's subtask field has an unexpected type".into()),
        };
        let count = {
            let children = subtasks.array_mut().ok_or("The parent task's subtask field is not an array")?;
            if source_index >= children.len() {
                return Err("The selected subtask position no longer exists".into());
            }
            children.remove(source_index);
            children.len()
        };
        set_count(parent, &count_field, count)?;
        let after = sync_hierarchy_links(&self.schema, self.container.header.version, &[&before], decoded.encode()?)?;
        let verified = decode_exact(&self.schema, &after, self.container.header.version)
            .map_err(|error| format!("Deleting this subtree would make the parent task invalid: {error}"))?;
        if verified.encode()? != after {
            return Err("The task root did not pass an exact byte round trip after deletion".into());
        }
        let mut deleted_ids = Vec::new();
        collect_task_ids(task_at(&decoded_before, path)?, &mut deleted_ids)?;
        let change = RootChange { pack, root, before: before.clone(), after: after.clone() };
        self.apply_changes(std::slice::from_ref(&change))?;
        self.id_floor = self.id_floor.max(deleted_ids.into_iter().max().unwrap_or(0));
        self.journal.record(EntryDetails {
            label: if preview.tasks == 1 { "Delete subquest".into() } else { format!("Delete subtree ({} quests)", preview.tasks) },
            task_id: preview.id,
            task_name: preview.name.clone(),
            field: "Task hierarchy".into(),
            old: format!("{} quest{}", preview.tasks, if preview.tasks == 1 { "" } else { "s" }),
            new: "Deleted".into(),
        }, vec![change]);
        Ok(TaskDeleteReport { state: self.edit_state(), pack, root, path: parent_path.to_vec(), id: preview.id, name: preview.name, tasks: preview.tasks })
    }

    pub fn undo(&mut self) -> Result<EditState, String> {
        let Some(changes) = self.journal.undo_changes() else { return Ok(self.edit_state()) };
        self.apply_changes(&changes)?;
        self.journal.commit_undo();
        Ok(self.edit_state())
    }

    pub fn redo(&mut self) -> Result<EditState, String> {
        let Some(changes) = self.journal.redo_changes() else { return Ok(self.edit_state()) };
        self.apply_changes(&changes)?;
        self.journal.commit_redo();
        Ok(self.edit_state())
    }

    /// Guards operations prepared in the UI against a task that moved or
    /// changed since: the task at `path` must still have `expected` as its ID.
    pub fn check_task_id(&self, pack: usize, root: usize, path: &[usize], expected: u32) -> Result<(), String> {
        let bytes = self.current_root(pack, root)?;
        let decoded = decode_exact(&self.schema, &bytes, self.container.header.version)?;
        let id = task_at(&decoded, path).and_then(task_heading).map(|(id, _)| id).ok();
        if id != Some(expected) {
            return Err(format!("Task {expected} is no longer at the selected position. Close this dialog and choose it again"));
        }
        Ok(())
    }

    pub fn revert_all(&mut self) -> Result<EditState, String> {
        if self.modified.is_empty() && self.added_roots.is_empty() {
            return Ok(self.edit_state());
        }
        let mut changes = self.modified.iter().map(|(&(pack, root), value)| RootChange {
            pack,
            root,
            before: value.current.clone(),
            after: value.original.clone(),
        }).collect::<Vec<_>>();
        for (&pack, roots) in &self.added_roots {
            let base = self.base_root_count(pack)?;
            for (index, bytes) in roots.iter().enumerate().rev() {
                changes.push(RootChange { pack, root: base + index, before: bytes.clone(), after: Vec::new() });
            }
        }
        self.apply_changes(&changes)?;
        self.journal.record(EntryDetails {
            label: "Revert all task edits".into(),
            task_id: 0,
            task_name: "All changed tasks".into(),
            field: "Multiple fields".into(),
            old: format!("{} changed root(s)", changes.len()),
            new: "Original bytes".into(),
        }, changes);
        Ok(self.edit_state())
    }

    pub(crate) fn current_root(&self, pack: usize, root: usize) -> Result<Vec<u8>, String> {
        self.current_root_optional(pack, root)?.ok_or_else(|| format!("Task root {}:{} does not exist", pack + 1, root + 1))
    }

    /// A range of `count` unused task IDs above every indexed ID and the
    /// session's `id_floor`. Requires the complete background index.
    fn fresh_task_ids(&self, count: usize) -> Result<(u64, u64), String> {
        let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
        if !index.indexed {
            return Err("Task IDs are still being indexed. Try cloning again in a moment".into());
        }
        let highest = index.entries.iter().map(|task| task.id).max().unwrap_or(0).max(self.id_floor);
        let first = u64::from(highest) + 1;
        let last = first.checked_add(count.saturating_sub(1) as u64).ok_or("Task ID range overflow")?;
        if last > u64::from(u32::MAX) {
            return Err("There are no free task IDs after the current maximum".into());
        }
        Ok((first, last))
    }

    pub(crate) fn root_count(&self, pack: usize) -> Result<usize, String> {
        Ok(self.base_root_count(pack)? + self.added_roots.get(&pack).map_or(0, Vec::len))
    }

    fn base_root_count(&self, pack: usize) -> Result<usize, String> {
        self.container.packs.get(pack).map(Pack::root_count).ok_or_else(|| format!("Task pack {} does not exist", pack + 1))
    }

    fn is_added_root(&self, pack: usize, root: usize) -> Result<bool, String> {
        Ok(root >= self.base_root_count(pack)? && self.current_root_optional(pack, root)?.is_some())
    }

    fn current_root_optional(&self, pack: usize, root: usize) -> Result<Option<Vec<u8>>, String> {
        let base = self.base_root_count(pack)?;
        if root < base {
            return Ok(Some(self.modified.get(&(pack, root)).map(|value| value.current.clone()).unwrap_or(self.container.root(pack, root)?)));
        }
        Ok(self.added_roots.get(&pack).and_then(|roots| roots.get(root - base)).cloned())
    }

    pub(crate) fn changed_root_keys(&self) -> Vec<(usize, usize)> {
        let mut keys = self.modified.keys().copied().collect::<HashSet<_>>();
        for (&pack, roots) in &self.added_roots {
            let base = self.container.packs.get(pack).map(Pack::root_count).unwrap_or(0);
            keys.extend((0..roots.len()).map(|index| (pack, base + index)));
        }
        keys.into_iter().collect()
    }

    fn apply_changes(&mut self, changes: &[RootChange]) -> Result<(), String> {
        for change in changes {
            let current = self.current_root_optional(change.pack, change.root)?;
            if (change.before.is_empty() && current.is_some()) || (!change.before.is_empty() && current.as_deref() != Some(change.before.as_slice())) {
                return Err(format!("Task root {}:{} changed since this edit was prepared", change.pack + 1, change.root + 1));
            }
            if !change.after.is_empty() {
                decode_exact(&self.schema, &change.after, self.container.header.version)?;
            }
        }
        for change in changes {
            self.apply_root(change.pack, change.root, change.after.clone())?;
        }
        Ok(())
    }

    fn apply_root(&mut self, pack: usize, root: usize, bytes: Vec<u8>) -> Result<(), String> {
        let base = self.base_root_count(pack)?;
        if root >= base {
            let slot = root - base;
            if bytes.is_empty() {
                let remove = self.added_roots.get_mut(&pack).ok_or("The appended task no longer exists")?;
                if slot + 1 != remove.len() {
                    return Err("Only the last appended top-level task can be removed".into());
                }
                remove.pop();
                if remove.is_empty() {
                    self.added_roots.remove(&pack);
                }
                self.remove_root_summary(pack, root);
                self.remove_root_from_index(pack, root)?;
                self.cache = None;
                return Ok(());
            }
            let node = decode_exact(&self.schema, &bytes, self.container.header.version)?;
            let appended = {
                let roots = self.added_roots.entry(pack).or_default();
                if slot > roots.len() {
                    return Err("A top-level task can only be appended after the current last task".into());
                }
                if slot == roots.len() {
                    roots.push(bytes.clone());
                    true
                } else {
                    roots[slot] = bytes.clone();
                    false
                }
            };
            if appended {
                self.insert_root_summary(pack, root, &node, bytes.len())?;
            }
            self.cache = Some(CachedRoot { pack, root, bytes: bytes.len(), node: node.clone(), original: node.clone() });
            return self.refresh_root(pack, root, &node, bytes.len());
        }
        if bytes.is_empty() {
            return Err("Removing an existing top-level task is not available yet".into());
        }
        let original = match self.modified.get(&(pack, root)) {
            Some(value) => value.original.clone(),
            None => self.container.root(pack, root)?,
        };
        if bytes == original {
            self.modified.remove(&(pack, root));
        } else {
            self.modified.insert((pack, root), ModifiedRoot { original: original.clone(), current: bytes.clone() });
        }
        let node = decode_exact(&self.schema, &bytes, self.container.header.version)?;
        let original_node = decode_exact(&self.schema, &original, self.container.header.version)?;
        self.cache = Some(CachedRoot { pack, root, bytes: bytes.len(), node: node.clone(), original: original_node });
        self.refresh_root(pack, root, &node, bytes.len())
    }

    fn insert_root_summary(&mut self, pack: usize, root: usize, node: &Node, byte_size: usize) -> Result<(), String> {
        let (id, name) = task_heading(node)?;
        let child_count = node.child("subtasks").map(|children| children.children().len()).unwrap_or(0);
        let at = self.summary.roots.iter().rposition(|candidate| candidate.pack == pack).map_or_else(
            || self.summary.roots.iter().position(|candidate| candidate.pack > pack).unwrap_or(self.summary.roots.len()),
            |index| index + 1,
        );
        self.summary.roots.insert(at, RootSummary { index: 0, pack, root, id, name, child_count, byte_size: byte_size as u64 });
        self.summary.root_count += 1;
        for (index, summary) in self.summary.roots.iter_mut().enumerate() {
            summary.index = index;
        }
        Ok(())
    }

    fn remove_root_summary(&mut self, pack: usize, root: usize) {
        self.summary.roots.retain(|candidate| candidate.pack != pack || candidate.root != root);
        self.summary.root_count = self.summary.roots.len();
        for (index, summary) in self.summary.roots.iter_mut().enumerate() {
            summary.index = index;
        }
    }

    fn remove_root_from_index(&mut self, pack: usize, root: usize) -> Result<(), String> {
        let mut index = self.search.write().map_err(|_| "Task search index lock poisoned")?;
        index.edited_roots.remove(&(pack, root));
        index.entries.retain(|entry| entry.pack != pack || entry.root != root);
        index.by_id.clear();
        for entry in index.entries.clone() {
            index.by_id.insert(entry.id, entry);
        }
        Ok(())
    }

    fn refresh_root(&mut self, pack: usize, root: usize, node: &Node, byte_size: usize) -> Result<(), String> {
        let (id, name) = task_heading(node)?;
        let child_count = node.child("subtasks").map(|children| children.children().len()).unwrap_or(0);
        if let Some(summary) = self.summary.roots.iter_mut().find(|candidate| candidate.pack == pack && candidate.root == root) {
            summary.id = id;
            summary.name = name.clone();
            summary.child_count = child_count;
            summary.byte_size = byte_size as u64;
        }
        let mut replacement = Vec::new();
        collect_search_entries(node, pack, root, Vec::new(), &mut replacement)?;
        let mut index = self.search.write().map_err(|_| "Task search index lock poisoned")?;
        index.edited_roots.insert((pack, root));
        index.entries.retain(|entry| entry.pack != pack || entry.root != root);
        index.entries.extend(replacement);
        index.by_id.clear();
        let entries = index.entries.clone();
        for entry in entries {
            index.by_id.insert(entry.id, entry);
        }
        Ok(())
    }
}

fn structural_paths(schema: &Schema, task: &Node) -> HashSet<Vec<String>> {
    let mut paths = HashSet::new();
    collect_structural_paths(schema, task, &task.ty, &[], &mut paths);
    paths
}

fn collect_structural_paths(schema: &Schema, node: &Node, ty: &FieldType, base: &[String], paths: &mut HashSet<Vec<String>>) {
    match ty {
        FieldType::Named { name } => {
            let Some(definition) = schema.structs.get(name) else { return };
            for field in &definition.fields {
                for condition in &field.when {
                    if let super::schema::Condition::Field { field, .. } = condition {
                        paths.insert(relative_path(base, field));
                    }
                }
                if let Some(count) = count_reference(&field.ty) {
                    paths.insert(relative_path(base, count));
                }
                if let Some(child) = node.child(&field.name) {
                    let mut child_base = base.to_vec();
                    child_base.push(field.name.clone());
                    collect_structural_paths(schema, child, &field.ty, &child_base, paths);
                }
            }
        }
        FieldType::FixedArray { item, .. } | FieldType::CountedArray { item, .. } => {
            for child in node.children() {
                let mut child_base = base.to_vec();
                child_base.push(child.name.clone());
                collect_structural_paths(schema, child, item, &child_base, paths);
            }
        }
        FieldType::RecursiveArray { target, .. } => {
            let item = FieldType::Named { name: target.clone() };
            for child in node.children() {
                let mut child_base = base.to_vec();
                child_base.push(child.name.clone());
                collect_structural_paths(schema, child, &item, &child_base, paths);
            }
        }
        _ => {}
    }
}

fn count_reference(ty: &FieldType) -> Option<&str> {
    match ty {
        FieldType::CountedUtf16 { count_field, .. }
        | FieldType::CountedBytes { count_field }
        | FieldType::CountedArray { count_field, .. }
        | FieldType::RecursiveArray { count_field, .. } => Some(count_field),
        _ => None,
    }
}

fn relative_path(base: &[String], reference: &str) -> Vec<String> {
    base.iter().cloned().chain(reference.split('.').map(str::to_string)).collect()
}

fn node_at<'a>(node: &'a Node, path: &[String]) -> Result<&'a Node, String> {
    let mut current = node;
    for part in path {
        current = current.child(part).ok_or_else(|| format!("Field path {} does not exist", path.join(".")))?;
    }
    Ok(current)
}

fn node_at_mut<'a>(node: &'a mut Node, path: &[String]) -> Result<&'a mut Node, String> {
    let shown = path.join(".");
    let mut current = node;
    for part in path {
        current = current.child_mut(part).ok_or_else(|| format!("Field path {shown} does not exist"))?;
    }
    Ok(current)
}

fn task_at_mut<'a>(root: &'a mut Node, path: &[usize]) -> Result<&'a mut Node, String> {
    let shown = display_path(path);
    let mut task = root;
    for &index in path {
        task = task
            .child_mut("subtasks")
            .and_then(|subtasks| subtasks.children_mut().get_mut(index))
            .ok_or_else(|| format!("Task path {shown} does not exist"))?;
    }
    Ok(task)
}

fn task_path_after_removal(destination: &[usize], source: &[usize]) -> Vec<usize> {
    let (&source_index, source_parent) = source.split_last().expect("move source paths are never empty");
    let mut adjusted = destination.to_vec();
    if adjusted.len() > source_parent.len()
        && adjusted[..source_parent.len()] == source_parent[..]
        && adjusted[source_parent.len()] > source_index
    {
        adjusted[source_parent.len()] -= 1;
    }
    adjusted
}

fn remove_subtask(root: &mut Node, path: &[usize]) -> Result<Node, String> {
    let (&index, parent_path) = path.split_last().ok_or("Root-task moving is not available yet")?;
    let parent = task_at_mut(root, parent_path)?;
    let subtasks = parent.child_mut("subtasks").ok_or("The parent task has no subtask array")?;
    let count_field = match &subtasks.ty {
        FieldType::RecursiveArray { count_field, .. } => count_field.clone(),
        _ => return Err("The parent task's subtask field has an unexpected type".into()),
    };
    let (moved, count) = {
        let children = subtasks.array_mut().ok_or("The parent task's subtask field is not an array")?;
        if index >= children.len() {
            return Err("The selected subtask position no longer exists".into());
        }
        let moved = children.remove(index);
        (moved, children.len())
    };
    set_count(parent, &count_field, count)?;
    Ok(moved)
}

fn append_subtask(parent: &mut Node, task: Node) -> Result<usize, String> {
    let subtasks = parent.child_mut("subtasks").ok_or("The destination task has no subtask array")?;
    let count_field = match &subtasks.ty {
        FieldType::RecursiveArray { count_field, .. } => count_field.clone(),
        _ => return Err("The destination task's subtask field has an unexpected type".into()),
    };
    let (index, count) = {
        let children = subtasks.array_mut().ok_or("The destination task's subtask field is not an array")?;
        let index = children.len();
        children.push(task);
        (index, children.len())
    };
    set_count(parent, &count_field, count)?;
    Ok(index)
}

fn verify_task_root(schema: &Schema, bytes: &[u8], version: u32, context: &str) -> Result<(), String> {
    let verified = decode_exact(schema, bytes, version).map_err(|error| format!("{context}: {error}"))?;
    if verified.encode()? != bytes {
        return Err(format!("{context}: exact byte round trip failed"));
    }
    Ok(())
}

/// Official tools store every task's parent, previous sibling, next sibling and
/// first child IDs in the last 16 bytes of its fixed block (`ATaskTemplFixedData`,
/// refreshed by `ATaskTempl::SynchID` before each save; top-level tasks hold zeros).
/// The game recomputes them after loading, but other editors read them, so
/// structural edits keep them exact.
const LINK_BYTES: usize = 16;

fn link_slots(task: &Node, parent: u32, prev: u32, next: u32, slots: &mut Vec<(usize, [u32; 4])>) -> Result<(), String> {
    let fixed = task.child("fixed").ok_or("Task record has no fixed block")?;
    if fixed.byte_len < LINK_BYTES {
        return Err("Task fixed block is too small for hierarchy links".into());
    }
    let id = task_heading(task)?.0;
    let children = task.child("subtasks").map(Node::children).unwrap_or_default();
    let ids = children.iter().map(|child| task_heading(child).map(|(id, _)| id)).collect::<Result<Vec<_>, _>>()?;
    slots.push((fixed.offset + fixed.byte_len - LINK_BYTES, [parent, prev, next, ids.first().copied().unwrap_or(0)]));
    for (index, child) in children.iter().enumerate() {
        let prev = if index > 0 { ids[index - 1] } else { 0 };
        link_slots(child, id, prev, ids.get(index + 1).copied().unwrap_or(0), slots)?;
    }
    Ok(())
}

fn read_links(bytes: &[u8], offset: usize) -> Option<[u32; 4]> {
    let block = bytes.get(offset..offset + LINK_BYTES)?;
    Some(std::array::from_fn(|index| u32::from_le_bytes(block[index * 4..index * 4 + 4].try_into().unwrap())))
}

/// True when every task in the root stores the links of its actual position.
fn hierarchy_links_consistent(schema: &Schema, bytes: &[u8], version: u32) -> bool {
    let Ok(node) = decode_exact(schema, bytes, version) else { return false };
    let mut slots = Vec::new();
    link_slots(&node, 0, 0, 0, &mut slots).is_ok() && slots.iter().all(|(offset, links)| read_links(bytes, *offset) == Some(*links))
}

/// Rewrites every task's hierarchy links in `after` when the root they came
/// from (`before`) kept them consistent; other roots are left untouched.
fn sync_hierarchy_links(schema: &Schema, version: u32, before: &[&[u8]], after: Vec<u8>) -> Result<Vec<u8>, String> {
    if !before.iter().all(|bytes| hierarchy_links_consistent(schema, bytes, version)) {
        return Ok(after);
    }
    let node = decode_exact(schema, &after, version)?;
    let mut slots = Vec::new();
    link_slots(&node, 0, 0, 0, &mut slots)?;
    let mut bytes = after;
    for (offset, links) in slots {
        for (index, value) in links.iter().enumerate() {
            bytes[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

fn parse_integer(value: &str) -> Result<i128, String> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        i128::from_str_radix(hex, 16).map_err(|_| format!("“{value}” is not a whole number"))
    } else {
        value.parse::<i128>().map_err(|_| format!("“{value}” is not a whole number"))
    }
}

fn parse_value(ty: &FieldType, input: &str) -> Result<Value, String> {
    Ok(match ty {
        FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 => {
            let value = parse_integer(input)?;
            Value::I64(i64::try_from(value).map_err(|_| "The value is outside the signed 64-bit range")?)
        }
        FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 => {
            let value = parse_integer(input)?;
            Value::U64(u64::try_from(value).map_err(|_| "The value must be a non-negative 64-bit integer")?)
        }
        FieldType::Bool8 => match input.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Value::Bool(true),
            "false" | "0" | "no" => Value::Bool(false),
            _ => return Err("Use true or false for a boolean value".into()),
        },
        FieldType::F32 => {
            let value = input.trim().parse::<f32>().map_err(|_| "This is not a valid 32-bit floating-point number")?;
            if !value.is_finite() { return Err("Floating-point values must be finite".into()) }
            Value::F32(value)
        }
        FieldType::F64 => {
            let value = input.trim().parse::<f64>().map_err(|_| "This is not a valid 64-bit floating-point number")?;
            if !value.is_finite() { return Err("Floating-point values must be finite".into()) }
            Value::F64(value)
        }
        FieldType::FixedUtf16 { .. } | FieldType::PrefixedUtf16 { .. } | FieldType::CountedUtf16 { .. } => Value::Text(crlf(input)),
        FieldType::Raw { len } => Value::Bytes(parse_hex(input, *len)?),
        _ => return Err("This binary type cannot be edited here".into()),
    })
}

fn crlf(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

fn parse_hex(input: &str, expected: usize) -> Result<Vec<u8>, String> {
    let cleaned = input.trim().strip_prefix("0x").or_else(|| input.trim().strip_prefix("0X")).unwrap_or(input.trim())
        .chars().filter(|character| !character.is_ascii_whitespace() && *character != '-' && *character != '_').collect::<String>();
    if cleaned.len() % 2 != 0 || !cleaned.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err("Raw values use pairs of hexadecimal digits, for example 01 00 FF 7A".into());
    }
    let bytes = (0..cleaned.len()).step_by(2)
        .map(|index| u8::from_str_radix(&cleaned[index..index + 2], 16).unwrap())
        .collect::<Vec<_>>();
    if bytes.len() != expected {
        return Err(format!("Raw value must contain exactly {expected} bytes; {} supplied", bytes.len()));
    }
    Ok(bytes)
}

fn set_count(parent: &mut Node, reference: &str, count: usize) -> Result<(), String> {
    let path = reference.split('.').map(str::to_string).collect::<Vec<_>>();
    let node = node_at_mut(parent, &path)?;
    let value = match node.value {
        Value::U64(_) => Value::U64(u64::try_from(count).map_err(|_| "Text length is too large")?),
        Value::I64(_) => Value::I64(i64::try_from(count).map_err(|_| "Text length is too large")?),
        _ => return Err(format!("{} is not an integer count", reference)),
    };
    node.set_value(value)
}

fn collect_task_ids(task: &Node, output: &mut Vec<u32>) -> Result<(), String> {
    output.push(task_heading(task)?.0);
    for child in task.child("subtasks").map(Node::children).unwrap_or_default() {
        collect_task_ids(child, output)?;
    }
    Ok(())
}

fn replace_own_task_ids(task: &mut Node, replacements: &HashMap<u32, u32>) -> Result<(), String> {
    let old = task_heading(task)?.0;
    let new = replacements.get(&old).copied().ok_or_else(|| format!("No replacement was assigned for task ID {old}"))?;
    let id = task.child_mut("fixed").and_then(|fixed| fixed.child_mut("id")).ok_or("Task record has no fixed.id field")?;
    let value = match id.value {
        Value::U64(_) => Value::U64(u64::from(new)),
        Value::I64(_) => Value::I64(i64::from(new)),
        _ => return Err("Task ID has an unexpected binary type".into()),
    };
    id.set_value(value)?;
    let children = task.child_mut("subtasks").map(Node::children_mut).unwrap_or_default();
    for child in children {
        replace_own_task_ids(child, replacements)?;
    }
    Ok(())
}

fn rewrite_internal_task_references(node: &mut Node, semantic: &str, replacements: &HashMap<u32, u32>) -> Result<(), String> {
    if node.children().is_empty() {
        if matches!(semantic, "task_id" | "new_task_id" | "terminate_task_ids") {
            let old = match node.value {
                Value::U64(value) => u32::try_from(value).ok(),
                Value::I64(value) => u32::try_from(value).ok(),
                _ => None,
            };
            if let Some(new) = old.and_then(|value| replacements.get(&value)).copied() {
                let value = match node.value {
                    Value::U64(_) => Value::U64(u64::from(new)),
                    Value::I64(_) => Value::I64(i64::from(new)),
                    _ => unreachable!(),
                };
                node.set_value(value)?;
            }
        }
        return Ok(());
    }
    for child in node.children_mut() {
        let child_semantic = if child.name.starts_with('[') { semantic.to_string() } else { child.name.to_ascii_lowercase() };
        rewrite_internal_task_references(child, &child_semantic, replacements)?;
    }
    Ok(())
}

fn delete_token(root: &[u8], path: &[usize]) -> String {
    let mut digest = Md5::new();
    digest.update(root);
    for index in path {
        digest.update((*index as u64).to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn collect_task_reference_fields(
    node: &Node,
    semantic: &str,
    path: Vec<String>,
    mut found: impl FnMut(u32, String),
) -> Result<(), String> {
    fn walk(node: &Node, semantic: &str, path: Vec<String>, found: &mut dyn FnMut(u32, String)) -> Result<(), String> {
        if node.children().is_empty() {
            if matches!(semantic, "task_id" | "new_task_id" | "terminate_task_ids") {
                let target = match node.value {
                    Value::U64(value) => u32::try_from(value).ok(),
                    Value::I64(value) => u32::try_from(value).ok(),
                    _ => None,
                };
                if let Some(target) = target {
                    found(target, path.iter().map(|part| label_path_part(part)).collect::<Vec<_>>().join(" › "));
                }
            }
            return Ok(());
        }
        for child in node.children() {
            let child_semantic = if child.name.starts_with('[') { semantic.to_string() } else { child.name.to_ascii_lowercase() };
            let mut child_path = path.clone();
            child_path.push(child.name.clone());
            walk(child, &child_semantic, child_path, found)?;
        }
        Ok(())
    }
    walk(node, semantic, path, &mut found)
}

fn label_path_part(part: &str) -> String {
    if part.starts_with('[') { part.into() } else { part.replace('_', " ") }
}

fn collect_search_entries(task: &Node, pack: usize, root: usize, path: Vec<usize>, output: &mut Vec<TaskSearchEntry>) -> Result<(), String> {
    let (id, name) = task_heading(task)?;
    let children = task.child("subtasks").map(Node::children).unwrap_or_default();
    let mut references = Vec::new();
    for field in task.children().iter().filter(|field| field.name != "subtasks") {
        collect_task_reference_fields(field, &field.name.to_ascii_lowercase(), vec![field.name.clone()], |target_id, field| {
            references.push(ProbedTaskReference { field, target_id });
        })?;
    }
    output.push(TaskSearchEntry { pack, root, path: path.clone(), id, name, child_count: children.len(), references });
    for (index, child) in children.iter().enumerate() {
        let mut child_path = path.clone();
        child_path.push(index);
        collect_search_entries(child, pack, root, child_path, output)?;
    }
    Ok(())
}

fn read_root_summaries(container: &TaskContainer, schema: &Schema) -> Result<Vec<RootSummary>, String> {
    schema.validate()?;
    let mut starts = Vec::with_capacity(container.packs.len());
    let mut start = 0usize;
    for pack in &container.packs {
        starts.push(start);
        start += pack.root_count();
    }

    let next_pack = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(container.packs.len());
    let mut by_pack = std::thread::scope(|scope| -> Result<Vec<Option<Vec<RootSummary>>>, String> {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(scope.spawn(|| {
                let mut scanned = Vec::new();
                loop {
                    let pack_index = next_pack.fetch_add(1, Ordering::Relaxed);
                    let Some(pack) = container.packs.get(pack_index) else { break };
                    scanned.push((pack_index, read_pack_summaries(pack, pack_index, starts[pack_index], schema, container.header.version)));
                }
                scanned
            }));
        }
        let mut by_pack = vec![None; container.packs.len()];
        for handle in handles {
            for (pack_index, summaries) in handle.join().map_err(|_| "Task summary worker panicked")? {
                by_pack[pack_index] = Some(summaries?);
            }
        }
        Ok(by_pack)
    })?;

    let mut result = Vec::with_capacity(container.header.root_count as usize);
    for (pack_index, summaries) in by_pack.iter_mut().enumerate() {
        result.append(summaries.as_mut().ok_or_else(|| format!("Task pack {} was not scanned", pack_index + 1))?);
    }
    Ok(result)
}

fn read_pack_summaries(pack: &Pack, pack_index: usize, first_index: usize, schema: &Schema, version: u32) -> Result<Vec<RootSummary>, String> {
    let data = std::fs::read(pack.path()).map_err(|error| format!("{}: {error}", pack.path().display()))?;
    let mut result = Vec::with_capacity(pack.root_count());
    for root in 0..pack.root_count() {
        let range = pack.root_range(root)?;
        if range.end - range.start < ROOT_HEADING_BYTES as u64 {
            return Err(format!("{}: root {} is too short to contain an ID and name", pack.path().display(), root + 1));
        }
        let start = usize::try_from(range.start).map_err(|_| format!("{}: root {} offset is too large", pack.path().display(), root + 1))?;
        let end = usize::try_from(range.end).map_err(|_| format!("{}: root {} end is too large", pack.path().display(), root + 1))?;
        let bytes = data.get(start..end).ok_or_else(|| format!("{}: root {} range is outside the pack", pack.path().display(), root + 1))?;
        let heading = &bytes[..ROOT_HEADING_BYTES];
        let count = probe_root_integer_validated(schema, bytes, version, "subtask_count")
            .map_err(|error| format!("{}: root {}: {error}", pack.path().display(), root + 1))?;
        let child_count = usize::try_from(count)
            .map_err(|_| format!("{}: root {} has an invalid subtask count {count}", pack.path().display(), root + 1))?;
        result.push(RootSummary {
            index: first_index + root,
            pack: pack_index,
            root,
            id: u32::from_le_bytes(heading[0..4].try_into().unwrap()),
            name: fixed_utf16(&heading[4..]),
            child_count,
            byte_size: range.end - range.start,
        });
    }
    Ok(result)
}

fn read_nested_index(container: &TaskContainer, schema: &Schema) -> Result<Vec<TaskSearchEntry>, String> {
    let next_pack = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(container.packs.len());
    let mut by_pack = std::thread::scope(|scope| -> Result<Vec<Option<Vec<TaskSearchEntry>>>, String> {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(scope.spawn(|| {
                let mut scanned = Vec::new();
                loop {
                    let pack_index = next_pack.fetch_add(1, Ordering::Relaxed);
                    let Some(pack) = container.packs.get(pack_index) else { break };
                    scanned.push((pack_index, read_pack_nested_index(pack, pack_index, schema, container.header.version)));
                }
                scanned
            }));
        }
        let mut by_pack = vec![None; container.packs.len()];
        for handle in handles {
            for (pack_index, entries) in handle.join().map_err(|_| "Task search worker panicked")? {
                by_pack[pack_index] = Some(entries?);
            }
        }
        Ok(by_pack)
    })?;
    let mut result = Vec::new();
    for (pack_index, entries) in by_pack.iter_mut().enumerate() {
        result.append(entries.as_mut().ok_or_else(|| format!("Task pack {} was not indexed", pack_index + 1))?);
    }
    Ok(result)
}

fn read_pack_nested_index(pack: &Pack, pack_index: usize, schema: &Schema, version: u32) -> Result<Vec<TaskSearchEntry>, String> {
    let data = std::fs::read(pack.path()).map_err(|error| format!("{}: {error}", pack.path().display()))?;
    let mut result = Vec::new();
    for root in 0..pack.root_count() {
        let range = pack.root_range(root)?;
        let start = usize::try_from(range.start).map_err(|_| format!("{}: root {} offset is too large", pack.path().display(), root + 1))?;
        let end = usize::try_from(range.end).map_err(|_| format!("{}: root {} end is too large", pack.path().display(), root + 1))?;
        let bytes = data.get(start..end).ok_or_else(|| format!("{}: root {} range is outside the pack", pack.path().display(), root + 1))?;
        let mut tasks = probe_task_index_validated(schema, bytes, version)
            .map_err(|error| format!("{}: root {}: {error}", pack.path().display(), root + 1))?;
        result.extend(tasks.drain(..).map(|task| TaskSearchEntry {
            pack: pack_index,
            root,
            path: task.path,
            id: task.id,
            name: task.name,
            child_count: task.child_count,
            references: task.references,
        }));
    }
    Ok(result)
}

fn fixed_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

fn task_at<'a>(root: &'a Node, path: &[usize]) -> Result<&'a Node, String> {
    let mut task = root;
    for &index in path {
        task = task
            .child("subtasks")
            .and_then(|subtasks| subtasks.children().get(index))
            .ok_or_else(|| format!("Task path {} does not exist", display_path(path)))?;
    }
    Ok(task)
}

fn display_path(path: &[usize]) -> String {
    if path.is_empty() { "root".into() } else { path.iter().map(|index| (index + 1).to_string()).collect::<Vec<_>>().join(".") }
}

fn task_heading(task: &Node) -> Result<(u32, String), String> {
    let fixed = task.child("fixed").ok_or("Task has no fixed header")?;
    let id = match &fixed.child("id").ok_or("Task has no ID")?.value {
        Value::U64(value) => u32::try_from(*value).map_err(|_| "Task ID is outside u32")?,
        _ => return Err("Task ID has an unexpected type".into()),
    };
    let name = match &fixed.child("name").ok_or("Task has no name")?.value {
        Value::Text(value) => value.clone(),
        _ => return Err("Task name has an unexpected type".into()),
    };
    Ok((id, name))
}

fn tree_view(task: &Node, path: Vec<usize>) -> Result<TreeNode, String> {
    let (id, name) = task_heading(task)?;
    let children = task
        .child("subtasks")
        .map(Node::children)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let mut child_path = path.clone();
            child_path.push(index);
            tree_view(child, child_path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TreeNode { id, name, path, children })
}

fn display_value(node: &Node) -> (Option<String>, Option<String>) {
    match &node.value {
        Value::I64(value) => (Some(value.to_string()), None),
        Value::U64(value) => (Some(value.to_string()), None),
        Value::F32(value) => (Some(format_float(*value as f64)), None),
        Value::F64(value) => (Some(format_float(*value)), None),
        Value::Bool(value) => (Some(value.to_string()), None),
        Value::Text(value) => (Some(value.clone()), None),
        Value::Bytes(bytes) => {
            let shown = if matches!(node.ty, FieldType::Raw { .. }) {
                bytes.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ")
            } else {
                hex_preview(bytes)
            };
            (Some(shown), raw_interpretation(bytes))
        }
        Value::Struct(values) => (Some(format!("{} fields", values.len())), None),
        Value::Array(values) => (Some(format!("{} items", values.len())), None),
    }
}

fn format_float(value: f64) -> String {
    if value.is_finite() { format!("{value:.7}").trim_end_matches('0').trim_end_matches('.').to_string() } else { value.to_string() }
}

fn hex_preview(bytes: &[u8]) -> String {
    let shown = bytes.iter().take(32).map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ");
    if bytes.len() > 32 { format!("{shown} …") } else if shown.is_empty() { "(empty)".into() } else { shown }
}

fn raw_interpretation(bytes: &[u8]) -> Option<String> {
    match bytes.len() {
        1 => Some(format!("u8 {} · i8 {}", bytes[0], bytes[0] as i8)),
        2 => {
            let raw: [u8; 2] = bytes.try_into().ok()?;
            Some(format!("u16 {} · i16 {}", u16::from_le_bytes(raw), i16::from_le_bytes(raw)))
        }
        4 => {
            let raw: [u8; 4] = bytes.try_into().ok()?;
            Some(format!("u32 {} · i32 {} · f32 {}", u32::from_le_bytes(raw), i32::from_le_bytes(raw), format_float(f32::from_le_bytes(raw) as f64)))
        }
        8 => {
            let raw: [u8; 8] = bytes.try_into().ok()?;
            Some(format!("u64 {} · i64 {} · f64 {}", u64::from_le_bytes(raw), i64::from_le_bytes(raw), format_float(f64::from_le_bytes(raw))))
        }
        _ => None,
    }
}

fn type_name(ty: &FieldType) -> String {
    match ty {
        FieldType::I8 => "int8".into(),
        FieldType::U8 => "uint8".into(),
        FieldType::Bool8 => "bool8".into(),
        FieldType::I16 => "int16".into(),
        FieldType::U16 => "uint16".into(),
        FieldType::I32 => "int32".into(),
        FieldType::U32 => "uint32".into(),
        FieldType::I64 => "int64".into(),
        FieldType::U64 => "uint64".into(),
        FieldType::F32 => "float32".into(),
        FieldType::F64 => "float64".into(),
        FieldType::FixedUtf16 { units } => format!("wstring[{units}]"),
        FieldType::PrefixedUtf16 { .. } => "wstring".into(),
        FieldType::CountedUtf16 { .. } => "counted wstring".into(),
        FieldType::Bytes { len } => format!("bytes[{len}]"),
        FieldType::CountedBytes { .. } => "counted bytes".into(),
        FieldType::Raw { len } => format!("raw[{len}]"),
        FieldType::Named { name } => name.clone(),
        FieldType::FixedArray { len, .. } => format!("array[{len}]"),
        FieldType::CountedArray { .. } => "counted array".into(),
        FieldType::RecursiveArray { .. } => "task array".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn decodes_root_heading_without_parsing_the_record() {
        let mut bytes = vec![0; ROOT_HEADING_BYTES];
        bytes[..4].copy_from_slice(&55u32.to_le_bytes());
        for (index, unit) in "Test task".encode_utf16().enumerate() {
            bytes[4 + index * 2..6 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 55);
        assert_eq!(fixed_utf16(&bytes[4..]), "Test task");
    }

    #[test]
    fn raw_values_offer_numeric_interpretations() {
        let value = raw_interpretation(&1.5f32.to_le_bytes()).unwrap();
        assert!(value.contains("f32 1.5"));
    }

    #[test]
    fn raw_edits_require_the_declared_width() {
        assert_eq!(parse_hex("01 02 FF", 3).unwrap(), vec![1, 2, 255]);
        assert!(parse_hex("01 02", 3).unwrap_err().contains("exactly 3 bytes"));
        assert!(parse_hex("not hex", 3).is_err());
    }

    #[test]
    fn task_text_uses_game_line_endings() {
        assert_eq!(crlf("one\ntwo\rthree\r\nfour"), "one\r\ntwo\r\nthree\r\nfour");
    }

    #[test]
    fn task_edit_undo_redo_and_revert_preserve_root_bytes() {
        let path = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        let mut document = TaskDocument::open(path).unwrap();
        let root = document.summary().roots[0].clone();
        let before = document.current_root(root.pack, root.root).unwrap();
        let original_name = document.task(root.pack, root.root, &[]).unwrap().name;
        let replacement = if original_name == "JD IDE edit test" { "JD IDE edit test 2" } else { "JD IDE edit test" };
        let state = document.edit_field(FieldEdit {
            pack: root.pack,
            root: root.root,
            task_path: Vec::new(),
            field_path: vec!["fixed".into(), "name".into()],
            value: replacement.into(),
        }).unwrap();
        assert_eq!(state.changed_roots.len(), 1);
        assert_eq!(document.task(root.pack, root.root, &[]).unwrap().name, replacement);
        let after = document.current_root(root.pack, root.root).unwrap();
        assert_ne!(after, before);
        assert_eq!(decode_exact(&document.schema, &after, document.container.header.version).unwrap().encode().unwrap(), after);

        document.undo().unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), before);
        assert!(document.edit_state().changed_roots.is_empty());
        document.redo().unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), after);
        document.revert_all().unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), before);
        document.undo().unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), after);
    }

    #[test]
    fn history_reverts_one_entry_without_undoing_later_ones() {
        let path = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        let mut document = TaskDocument::open(path).unwrap();
        let roots = document.summary().roots;
        let (first, second) = (roots[0].clone(), roots[1].clone());
        let rename = |document: &mut TaskDocument, root: &RootSummary, name: &str| {
            document.edit_field(FieldEdit { pack: root.pack, root: root.root, task_path: Vec::new(), field_path: vec!["fixed".into(), "name".into()], value: name.into() }).unwrap();
        };
        let first_before = document.current_root(first.pack, first.root).unwrap();
        rename(&mut document, &first, "JD IDE revert A");
        rename(&mut document, &second, "JD IDE revert B");
        let second_after = document.current_root(second.pack, second.root).unwrap();
        let history = document.history();
        assert_eq!(history.len(), 2);
        let (newest, oldest) = (history[0].id, history[1].id);
        assert!(history.iter().all(|entry| entry.revert_blocked.is_none()));

        // The older edit goes back on its own; the later one stays.
        document.revert_entry(oldest).unwrap();
        assert_eq!(document.current_root(first.pack, first.root).unwrap(), first_before);
        assert_eq!(document.current_root(second.pack, second.root).unwrap(), second_after);
        let history = document.history();
        assert_eq!(history.len(), 2, "a revert is not listed as an entry of its own");
        assert!(history.iter().find(|entry| entry.id == oldest).unwrap().reverted_at.is_some());
        // Reverting a reverted entry does nothing, so there is no back-and-forth.
        document.revert_entry(oldest).unwrap();
        assert_eq!(document.current_root(first.pack, first.root).unwrap(), first_before);
        // Undo takes the revert back.
        document.undo().unwrap();
        assert_ne!(document.current_root(first.pack, first.root).unwrap(), first_before);
        assert!(document.history().iter().all(|entry| entry.reverted_at.is_none()));

        // A later edit of the same root blocks reverting the earlier one.
        rename(&mut document, &second, "JD IDE revert C");
        let blocked = document.history().into_iter().find(|entry| entry.id == newest).unwrap().revert_blocked;
        assert!(blocked.is_some_and(|reason| reason.contains("JD IDE") || reason.contains("Edit")), "the reason names the later edit");
        assert!(document.revert_entry(newest).is_err());
    }

    #[test]
    fn variable_text_edit_rebuilds_and_undoes_the_containing_root() {
        let path = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        fn variable_text(fields: &[FieldView]) -> Option<FieldView> {
            fields.iter().find_map(|field| {
                if field.editable && matches!(field.ty.as_str(), "wstring" | "counted wstring") {
                    Some(field.clone())
                } else {
                    variable_text(&field.children)
                }
            })
        }
        let mut document = TaskDocument::open(path).unwrap();
        let mut found = None;
        for root in document.summary().roots.into_iter().take(500) {
            let detail = document.task(root.pack, root.root, &[]).unwrap();
            if let Some(field) = variable_text(&detail.fields) {
                found = Some((root, field));
                break;
            }
        }
        let (root, field) = found.expect("the real task fixture should contain variable-length text");
        let before = document.current_root(root.pack, root.root).unwrap();
        let replacement = if field.value.as_deref() == Some("JD IDE variable text") { "x" } else { "JD IDE variable text" };
        document.edit_field(FieldEdit {
            pack: root.pack,
            root: root.root,
            task_path: Vec::new(),
            field_path: field.path,
            value: replacement.into(),
        }).unwrap();
        let after = document.current_root(root.pack, root.root).unwrap();
        assert_ne!(after, before);
        assert_ne!(after.len(), before.len());
        assert_eq!(decode_exact(&document.schema, &after, document.container.header.version).unwrap().encode().unwrap(), after);
        document.undo().unwrap();
        assert_eq!(document.current_root(root.pack, root.root).unwrap(), before);
    }

    #[test]
    fn moving_a_subtask_adjusts_later_sibling_destinations() {
        assert_eq!(task_path_after_removal(&[2], &[1]), vec![1]);
        assert_eq!(task_path_after_removal(&[2, 4], &[1]), vec![1, 4]);
        assert_eq!(task_path_after_removal(&[1], &[1]), vec![1]);
        assert_eq!(task_path_after_removal(&[0, 2], &[0, 1]), vec![0, 1]);
        assert_eq!(task_path_after_removal(&[1, 0], &[0, 1]), vec![1, 0]);
    }

    #[test]
    fn browses_first_root_of_real_supported_files() {
        let samples = [
            (r"E:/Games/XtremeJade/element/data/tasks.data", 165),
            (r"E:/Games/ForsakenJD/element/data/tasks.data", 172),
            (r"E:/Games/Elite Jade Dynasty - HDN/element/data/tasks.data", 184),
        ];
        for (path, version) in samples {
            if !Path::new(path).is_file() {
                continue;
            }
            let mut document = TaskDocument::open(path).unwrap();
            assert_eq!(document.summary().version, version);
            assert_eq!(document.summary().roots.len(), document.summary().root_count);
            let detail = document.task(0, 0, &[]).unwrap();
            assert_eq!(detail.id, document.summary().roots[0].id);
            assert_eq!(detail.name, document.summary().roots[0].name);
            assert_eq!(detail.tree.children.len(), document.summary().roots[0].child_count);
            assert!(!detail.fields.is_empty());

            let parent = document.summary().roots.into_iter().find(|root| root.child_count > 0)
                .expect("real task set should contain a root with subtasks");
            let parent_detail = document.task(parent.pack, parent.root, &[]).unwrap();
            assert_eq!(parent_detail.tree.children.len(), parent.child_count);

            // Full real-file tests scan four large task sets in parallel. Give
            // the background index enough room when the same disk is saturated.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            let report = loop {
                let report = document.search("__no_such_task__", 1);
                assert!(report.error.is_none(), "{:?}", report.error);
                if report.indexed {
                    break document.search("", 50_000);
                }
                assert!(std::time::Instant::now() < deadline, "background task index timed out");
                std::thread::sleep(std::time::Duration::from_millis(50));
            };
            assert!(report.total > document.summary().root_count);
            assert!(report.matches.iter().any(|task| !task.path.is_empty()));
            if version == 165 {
                let roots_before_clone = document.summary().root_count;
                let clone_pack = if document.container.packs[parent.pack].root_count() < ROOTS_PER_PACK {
                    parent.pack
                } else {
                    document.container.packs.iter().position(|pack| pack.root_count() < ROOTS_PER_PACK).expect("real task set should have a pack with room for one clone")
                };
                let clone_root_index = document.container.packs[clone_pack].root_count();
                let clone_root = document.clone_root_task(parent.pack, parent.root).unwrap();
                assert_eq!(clone_root.pack, clone_pack);
                assert_eq!(clone_root.root, clone_root_index);
                assert_eq!(clone_root.path, Vec::<usize>::new());
                assert_eq!(document.summary().root_count, roots_before_clone + 1);
                let cloned_root_bytes = document.current_root(clone_root.pack, clone_root.root).unwrap();
                assert_eq!(decode_exact(&document.schema, &cloned_root_bytes, version).unwrap().encode().unwrap(), cloned_root_bytes);
                assert!(hierarchy_links_consistent(&document.schema, &document.current_root(parent.pack, parent.root).unwrap(), version));
                assert!(hierarchy_links_consistent(&document.schema, &cloned_root_bytes, version), "a cloned tree links its children to the new IDs");
                assert_eq!(document.task(clone_root.pack, clone_root.root, &[]).unwrap().id, clone_root.id);
                document.undo().unwrap();
                assert_eq!(document.summary().root_count, roots_before_clone);
                assert!(document.current_root(clone_root.pack, clone_root.root).is_err());

                let source_before_move = document.current_root(parent.pack, parent.root).unwrap();
                let moved_within_root = document.move_subtask(parent.pack, parent.root, &[0], parent.pack, parent.root, &[]).unwrap();
                assert_eq!(moved_within_root.path, vec![parent.child_count - 1]);
                let source_after_move = document.current_root(parent.pack, parent.root).unwrap();
                assert_ne!(source_after_move, source_before_move);
                assert_eq!(decode_exact(&document.schema, &source_after_move, version).unwrap().encode().unwrap(), source_after_move);
                assert!(hierarchy_links_consistent(&document.schema, &source_after_move, version), "moving updates sibling and first-child links");
                document.undo().unwrap();
                assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), source_before_move);

                let destination = document.summary().roots.into_iter().find(|root| root.pack != parent.pack || root.root != parent.root)
                    .expect("real task set should contain more than one root");
                let destination_before_move = document.current_root(destination.pack, destination.root).unwrap();
                let moved_across_roots = document.move_subtask(parent.pack, parent.root, &[0], destination.pack, destination.root, &[]).unwrap();
                assert_eq!(moved_across_roots.pack, destination.pack);
                assert_eq!(moved_across_roots.root, destination.root);
                assert_eq!(moved_across_roots.path, vec![destination.child_count]);
                let source_after_cross_move = document.current_root(parent.pack, parent.root).unwrap();
                let destination_after_cross_move = document.current_root(destination.pack, destination.root).unwrap();
                assert_eq!(decode_exact(&document.schema, &source_after_cross_move, version).unwrap().encode().unwrap(), source_after_cross_move);
                assert_eq!(decode_exact(&document.schema, &destination_after_cross_move, version).unwrap().encode().unwrap(), destination_after_cross_move);
                assert!(hierarchy_links_consistent(&document.schema, &source_after_cross_move, version));
                assert!(hierarchy_links_consistent(&document.schema, &destination_after_cross_move, version));
                document.undo().unwrap();
                assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), source_before_move);
                assert_eq!(document.current_root(destination.pack, destination.root).unwrap(), destination_before_move);

                let before = document.current_root(parent.pack, parent.root).unwrap();
                let source = document.task(parent.pack, parent.root, &[0]).unwrap();
                let cloned = document.clone_subtask(parent.pack, parent.root, &[0]).unwrap();
                assert_ne!(cloned.id, source.id);
                assert_eq!(cloned.name, source.name);
                assert!(cloned.tasks >= 1);
                let copy = document.task(parent.pack, parent.root, &cloned.path).unwrap();
                assert_eq!(copy.id, cloned.id);
                let after = document.current_root(parent.pack, parent.root).unwrap();
                assert_ne!(after, before);
                assert_eq!(decode_exact(&document.schema, &after, version).unwrap().encode().unwrap(), after);
                assert!(hierarchy_links_consistent(&document.schema, &after, version), "a cloned subtree links into its new siblings");
                assert!(document.check_task_id(parent.pack, parent.root, &cloned.path, cloned.id).is_ok());
                assert!(document.check_task_id(parent.pack, parent.root, &cloned.path, source.id).is_err());
                let preview = document.delete_subtask_preview(parent.pack, parent.root, &cloned.path).unwrap();
                assert_eq!(preview.id, cloned.id);
                assert_eq!(preview.tasks, cloned.tasks);
                assert!(document.delete_subtask(parent.pack, parent.root, &cloned.path, "stale", true).is_err());
                let deleted = document.delete_subtask(parent.pack, parent.root, &cloned.path, &preview.token, true).unwrap();
                assert_eq!(deleted.path, cloned.path[..cloned.path.len() - 1]);
                assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), before);
                document.undo().unwrap();
                assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), after);
                document.undo().unwrap();
                assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), before);

                // IDs handed out earlier stay reserved after the clone was deleted and undone.
                let again = document.clone_subtask(parent.pack, parent.root, &[0]).unwrap();
                assert!(again.id > cloned.id + cloned.tasks as u32 - 1, "fresh IDs must not reuse {} after its clone was removed", cloned.id);
                document.undo().unwrap();
                assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), before);
            }
        }
    }
}


