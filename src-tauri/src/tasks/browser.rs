//! Read-only view models for the tasks.data browser.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::{client::Resources, elements::Document};

use super::container::{Pack, TaskContainer};
use super::edit::{ChangedRoot, EditState, EntryDetails, HistoryEntry, Journal, RootChange};
use super::schema::{decode_exact, probe_root_integer_validated, probe_task_index_validated, FieldType, Node, Schema, Value};
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
    journal: Journal,
    pub(crate) disk: HashMap<std::path::PathBuf, super::save::DiskStamp>,
    pub(crate) backed_up: HashSet<std::path::PathBuf>,
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
                    for task in subtasks {
                        if index.edited_roots.contains(&(task.pack, task.root)) {
                            continue;
                        }
                        index.by_id.insert(task.id, task.clone());
                        index.entries.push(task);
                    }
                    index.indexed = true;
                }
                Err(error) => index.error = Some(error),
            }
        });
        let disk = super::save::stamps(&container);
        Ok(Self { container, schema, summary, search, cache: None, modified: HashMap::new(), journal: Journal::default(), disk, backed_up: HashSet::new() })
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
                task.id == id
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
            let original_bytes = self.modified.get(&(pack, root)).map(|root| root.original.clone()).unwrap_or_else(|| bytes.clone());
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
            let target = self.search.read().ok().and_then(|index| index.by_id.get(&id).cloned());
            return Some(FieldReference {
                kind: "task".into(),
                id,
                label: target.as_ref().map(|task| task.name.clone()).unwrap_or_else(|| "Task not found".into()),
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
        self.journal.state(self.modified.keys().map(|&(pack, root)| ChangedRoot { pack, root }).collect())
    }

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.journal.history()
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

    pub fn undo(&mut self) -> Result<EditState, String> {
        let Some(changes) = self.journal.undo() else { return Ok(self.edit_state()) };
        self.apply_changes(&changes)?;
        Ok(self.edit_state())
    }

    pub fn redo(&mut self) -> Result<EditState, String> {
        let Some(changes) = self.journal.redo() else { return Ok(self.edit_state()) };
        self.apply_changes(&changes)?;
        Ok(self.edit_state())
    }

    pub fn revert_all(&mut self) -> Result<EditState, String> {
        if self.modified.is_empty() {
            return Ok(self.edit_state());
        }
        let changes = self.modified.iter().map(|(&(pack, root), value)| RootChange {
            pack,
            root,
            before: value.current.clone(),
            after: value.original.clone(),
        }).collect::<Vec<_>>();
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
        match self.modified.get(&(pack, root)) {
            Some(value) => Ok(value.current.clone()),
            None => self.container.root(pack, root),
        }
    }

    fn apply_changes(&mut self, changes: &[RootChange]) -> Result<(), String> {
        for change in changes {
            if self.current_root(change.pack, change.root)? != change.before {
                return Err(format!("Task root {}:{} changed since this edit was prepared", change.pack + 1, change.root + 1));
            }
            decode_exact(&self.schema, &change.after, self.container.header.version)?;
        }
        for change in changes {
            self.apply_root(change.pack, change.root, change.after.clone())?;
        }
        Ok(())
    }

    fn apply_root(&mut self, pack: usize, root: usize, bytes: Vec<u8>) -> Result<(), String> {
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

fn label_path_part(part: &str) -> String {
    if part.starts_with('[') { part.into() } else { part.replace('_', " ") }
}

fn collect_search_entries(task: &Node, pack: usize, root: usize, path: Vec<usize>, output: &mut Vec<TaskSearchEntry>) -> Result<(), String> {
    let (id, name) = task_heading(task)?;
    let children = task.child("subtasks").map(Node::children).unwrap_or_default();
    output.push(TaskSearchEntry { pack, root, path: path.clone(), id, name, child_count: children.len() });
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
        result.extend(tasks.drain(1..).map(|task| TaskSearchEntry {
            pack: pack_index,
            root,
            path: task.path,
            id: task.id,
            name: task.name,
            child_count: task.child_count,
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
        }
    }
}
