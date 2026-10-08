//! Versioned JSON export and import of tasks.
//!
//! An export lists tasks by ID with their editable values by field path
//! (\`fixed.name\`, \`fixed.premise_tasks[0]\`). Top-level tasks also carry
//! their complete root bytes (\`_raw\`, the task and all its subtasks), so an
//! import into another task set of the same version and layout can add them.
//! Import updates existing tasks by ID with the inspector's rules, adds
//! missing top-level tasks from \`_raw\`, and applies everything as one undo
//! step after a preview whose token guards against stale input or data.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as Json};

use super::browser::{collect_task_ids, editable_type, label_path_part, locked_field, set_task_field, structural_paths, task_at, task_at_mut, task_heading, verify_task_root, TaskDocument};
use super::container::ROOTS_PER_PACK;
use super::edit::{EditState, EntryDetails, RootChange};
use super::schema::{decode_exact, FieldType, Node, Schema, Value};

pub const FORMAT: &str = "jdide-tasks";
pub const FORMAT_VERSION: u64 = 1;
/// Rows listed in a preview (the rest are only counted).
const PREVIEW_ROWS: usize = 200;
const PREVIEW_ISSUES: usize = 100;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTarget {
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub path: String,
    pub tasks: usize,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportChange {
    pub source_row: usize,
    pub id: u32,
    pub name: String,
    pub field: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAddition {
    pub source_row: usize,
    pub id: u32,
    pub name: String,
    /// Zero-based pack the task is appended to.
    pub pack: usize,
    /// Tasks in the added tree, the top-level task included.
    pub tasks: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportIssue {
    pub source_row: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub token: String,
    pub source_version: u32,
    /// Input rows.
    pub total: usize,
    pub changing: usize,
    pub adding: usize,
    pub unchanged: usize,
    pub rejected: usize,
    /// Field changes over all changing rows.
    pub fields: usize,
    pub changes: Vec<ImportChange>,
    pub additions: Vec<ImportAddition>,
    pub issues: Vec<ImportIssue>,
    /// Set once the import was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EditState>,
}

/// "fixed.premise_tasks[0]" for a field path from a task.
pub(crate) fn dotted(path: &[String]) -> String {
    let mut text = String::new();
    for part in path {
        if !part.starts_with('[') && !text.is_empty() {
            text.push('.');
        }
        text.push_str(part);
    }
    text
}

/// The inverse of \`dotted\`.
pub(crate) fn field_path(dotted: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    for segment in dotted.split('.') {
        let (name, mut rest) = segment.split_once('[').map_or((segment, ""), |(name, rest)| (name, rest));
        if name.is_empty() || name.contains(']') {
            return Err(format!("\"{dotted}\" is not a field path such as fixed.name or fixed.premise_tasks[0]"));
        }
        parts.push(name.to_string());
        while !rest.is_empty() {
            let (index, after) = rest.split_once(']').ok_or_else(|| format!("\"{dotted}\" has an unclosed ["))?;
            index.parse::<usize>().map_err(|_| format!("\"{dotted}\" has an invalid array index"))?;
            parts.push(format!("[{index}]"));
            rest = after.strip_prefix('[').unwrap_or(after);
            if !after.is_empty() && !after.starts_with('[') {
                return Err(format!("\"{dotted}\" is not a field path"));
            }
        }
    }
    Ok(parts)
}

fn label(path: &[String]) -> String {
    path.iter().map(|part| label_path_part(part)).collect::<Vec<_>>().join(" › ")
}

/// A value as JSON: integers as numbers (strings beyond 2^53), floats in their
/// shortest exact form, booleans, text, and raw bytes as hex.
pub(crate) fn json_value(node: &Node) -> Option<Json> {
    const SAFE: u64 = 1 << 53;
    Some(match &node.value {
        Value::U64(value) if *value < SAFE => json!(value),
        Value::U64(value) => json!(value.to_string()),
        Value::I64(value) if value.unsigned_abs() < SAFE => json!(value),
        Value::I64(value) => json!(value.to_string()),
        Value::F32(value) => serde_json::Number::from_f64(value.to_string().parse::<f64>().ok()?).map(Json::Number)?,
        Value::F64(value) => serde_json::Number::from_f64(*value).map(Json::Number)?,
        Value::Bool(value) => json!(value),
        Value::Text(text) => json!(text),
        Value::Bytes(bytes) => json!(bytes.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ")),
        Value::Struct(_) | Value::Array(_) => return None,
    })
}

/// The text an imported JSON value stands for, as the inspector would accept it.
fn input_text(value: &Json) -> Result<String, String> {
    Ok(match value {
        Json::Number(number) => number.to_string(),
        Json::Bool(value) => value.to_string(),
        Json::String(text) => text.clone(),
        Json::Null => return Err("null is not a value; leave the field out to keep it".into()),
        _ => return Err("Use a number, true/false or text".into()),
    })
}

/// Every editable named value of one task (not its subtasks), by dotted path.
/// Raw fields of unknown meaning are left out; \`_raw\` keeps them.
fn task_fields(schema: &Schema, task: &Node) -> Map<String, Json> {
    fn walk(node: &Node, path: &mut Vec<String>, locked: &HashSet<Vec<String>>, output: &mut Map<String, Json>) {
        if node.children().is_empty() {
            if editable_type(&node.ty) && !matches!(node.ty, FieldType::Raw { .. }) && locked_field(locked, path).is_none() {
                if let Some(value) = json_value(node) {
                    output.insert(dotted(path), value);
                }
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
    let mut output = Map::new();
    for field in task.children().iter().filter(|field| field.name != "subtasks") {
        walk(field, &mut vec![field.name.clone()], &locked, &mut output);
    }
    output
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("_raw must be the task's bytes as hexadecimal digits".into());
    }
    Ok((0..text.len()).step_by(2).map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap()).collect())
}

/// Identifies the binary layout: equal versions can still differ through user layouts.
pub(crate) fn schema_digest(schema: &Schema) -> Result<String, String> {
    let mut hash = Md5::new();
    hash.update(serde_json::to_vec(schema).map_err(|error| error.to_string())?);
    Ok(format!("{:x}", hash.finalize()))
}

/// Where an added tree's tasks are, by ID.
fn tree_positions(task: &Node, path: &mut Vec<usize>, output: &mut Vec<(u32, Vec<usize>)>) -> Result<(), String> {
    output.push((task_heading(task)?.0, path.clone()));
    for (index, child) in task.child("subtasks").map(Node::children).unwrap_or_default().iter().enumerate() {
        path.push(index);
        tree_positions(child, path, output)?;
        path.pop();
    }
    Ok(())
}

struct Plan {
    report: ImportReport,
    changes: Vec<RootChange>,
    highest_added: u32,
}

impl TaskDocument {
    /// Writes the given tasks (and with \`subtrees\` every task below them) as JSON.
    pub fn export_json(&self, targets: &[ExportTarget], subtrees: bool, target: &Path) -> Result<ExportReport, String> {
        let version = self.container.header.version;
        let mut decoded = HashMap::<(usize, usize), Node>::new();
        let mut seen = HashSet::new();
        let mut rows = Vec::new();
        for target_task in targets {
            let key = (target_task.pack, target_task.root);
            if !decoded.contains_key(&key) {
                let bytes = self.current_root(target_task.pack, target_task.root)?;
                decoded.insert(key, decode_exact(&self.schema, &bytes, version)?);
            }
            let root = &decoded[&key];
            let mut positions = Vec::new();
            let start = task_at(root, &target_task.path)?;
            if subtrees {
                tree_positions(start, &mut target_task.path.clone(), &mut positions)?;
            } else {
                positions.push((task_heading(start)?.0, target_task.path.clone()));
            }
            for (_, path) in positions {
                if !seen.insert((target_task.pack, target_task.root, path.clone())) {
                    continue;
                }
                let task = task_at(root, &path)?;
                let (id, name) = task_heading(task)?;
                let mut row = Map::new();
                row.insert("id".into(), json!(id));
                row.insert("name".into(), json!(name));
                row.insert("_pack".into(), json!(target_task.pack));
                row.insert("_root".into(), json!(target_task.root));
                row.insert("_path".into(), json!(path));
                row.insert("fields".into(), Json::Object(task_fields(&self.schema, task)));
                if path.is_empty() {
                    row.insert("_raw".into(), json!(hex(&self.current_root(target_task.pack, target_task.root)?)));
                }
                rows.push(Json::Object(row));
            }
        }
        let document = json!({
            "format": FORMAT,
            "formatVersion": FORMAT_VERSION,
            "taskVersion": version,
            "schemaDigest": schema_digest(&self.schema)?,
            "source": self.summary.path,
            "exported": chrono::Local::now().to_rfc3339(),
            "tasks": rows,
        });
        let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
        std::fs::write(target, &bytes).map_err(|error| format!("Could not write {}: {error}", target.display()))?;
        Ok(ExportReport { path: target.display().to_string(), tasks: rows.len(), bytes: bytes.len() as u64 })
    }

    /// Previews an import, or applies it when \`token\` matches the preview.
    pub fn import_json(&mut self, path: &Path, token: Option<&str>) -> Result<ImportReport, String> {
        let input = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let plan = self.import_plan(&input)?;
        let Some(token) = token else { return Ok(plan.report) };
        if token != plan.report.token {
            return Err("The import file or the open task set changed since the preview. Refresh the preview before applying.".into());
        }
        let label = format!("Import JSON ({} updated, {} added)", plan.report.changing, plan.report.adding);
        self.apply_plan(plan, label, "Imported tasks")
    }

    /// Copies field values and whole top-level trees from compared tasks (rows
    /// built by `ComparedTasks::copy_rows`) as one undo step. Trees are added
    /// only when `same_layout`.
    pub fn copy_rows(&mut self, rows: &[Json], same_layout: bool) -> Result<ImportReport, String> {
        let plan = self.plan_rows(rows, same_layout, String::new(), self.container.header.version)?;
        let label = format!("Copy from compared file ({} updated, {} added)", plan.report.changing, plan.report.adding);
        self.apply_plan(plan, label, "Compared tasks")
    }

    fn apply_plan(&mut self, plan: Plan, label: String, task_name: &str) -> Result<ImportReport, String> {
        let Plan { mut report, changes, highest_added } = plan;
        if !changes.is_empty() {
            self.apply_changes(&changes)?;
            self.id_floor = self.id_floor.max(highest_added);
            self.journal.record(EntryDetails {
                label,
                task_id: 0,
                task_name: task_name.into(),
                field: "Multiple fields".into(),
                old: format!("{} task root(s)", changes.len()),
                new: format!("{} field change(s), {} added task tree(s)", report.fields, report.adding),
            }, changes);
        }
        report.state = Some(self.edit_state());
        Ok(report)
    }

    fn import_plan(&self, input: &[u8]) -> Result<Plan, String> {
        let version = self.container.header.version;
        let document: Json = serde_json::from_slice(input).map_err(|error| format!("This is not valid JSON: {error}"))?;
        if document.get("format").and_then(Json::as_str) != Some(FORMAT) {
            return Err("This is not a JD IDE tasks.data export (format \"jdide-tasks\"). Export tasks from the tasks.data editor first.".into());
        }
        if document.get("formatVersion").and_then(Json::as_u64) != Some(FORMAT_VERSION) {
            return Err(format!("Unsupported export format version; this JD IDE reads version {FORMAT_VERSION}."));
        }
        let source_version = document.get("taskVersion").and_then(Json::as_u64).ok_or("The export has no taskVersion")? as u32;
        if source_version != version {
            return Err(format!("The export is from tasks.data v{source_version}; the open task set is v{version}. JD IDE does not convert tasks between versions."));
        }
        let digest = schema_digest(&self.schema)?;
        if document.get("schemaDigest").and_then(Json::as_str) != Some(digest.as_str()) {
            return Err("The export was made with a different task layout for this version. Import requires the same layout.".into());
        }
        let rows = document.get("tasks").and_then(Json::as_array).ok_or("The export has no tasks array")?;

        let mut hash = Md5::new();
        hash.update(input);
        hash.update(self.summary.path.as_bytes());
        hash.update(format!("{:?}", self.journal.generation()).as_bytes());
        hash.update(digest.as_bytes());
        let token = format!("{:x}", hash.finalize());
        self.plan_rows(rows, true, token, source_version)
    }

    /// Plans field updates by task ID and, when `allow_additions`, missing
    /// top-level trees from `_raw`. Rows with any error are skipped whole.
    fn plan_rows(&self, rows: &[Json], allow_additions: bool, token: String, source_version: u32) -> Result<Plan, String> {
        let version = self.container.header.version;
        let index = self.search.read().map_err(|_| "Task search index lock poisoned")?;
        if let Some(error) = &index.error {
            return Err(format!("The task index could not be built: {error}"));
        }
        if !index.indexed {
            return Err("Tasks are still being indexed. Try again in a moment".into());
        }
        let mut existing = HashMap::<u32, Vec<(usize, usize, Vec<usize>)>>::new();
        for task in &index.entries {
            existing.entry(task.id).or_default().push((task.pack, task.root, task.path.clone()));
        }
        drop(index);

        let mut report = ImportReport { token, source_version, total: rows.len(), changing: 0, adding: 0, unchanged: 0, rejected: 0, fields: 0, changes: Vec::new(), additions: Vec::new(), issues: Vec::new(), state: None };
        let reject = |report: &mut ImportReport, source_row: usize, id: Option<u32>, message: String| {
            report.rejected += 1;
            if report.issues.len() < PREVIEW_ISSUES {
                report.issues.push(ImportIssue { source_row, id, message });
            }
        };

        // Identities first, so every row of a duplicated ID is rejected.
        let ids = rows.iter().map(|row| row.get("id").and_then(Json::as_u64).and_then(|id| u32::try_from(id).ok())).collect::<Vec<_>>();
        let mut occurrences = HashMap::<u32, usize>::new();
        for id in ids.iter().flatten() {
            *occurrences.entry(*id).or_default() += 1;
        }
        let mut valid = vec![true; rows.len()];
        for (row, id) in ids.iter().enumerate() {
            let message = match id {
                None => Some("Missing or invalid task \"id\"".to_string()),
                Some(id) if occurrences[id] > 1 => Some(format!("Task ID {id} appears in {} input rows", occurrences[id])),
                Some(id) if existing.get(id).is_some_and(|places| places.len() > 1) => Some(format!("Task ID {id} is used by several tasks in the open set, so the row cannot be matched")),
                _ => None,
            };
            if let Some(message) = message {
                valid[row] = false;
                reject(&mut report, row + 1, *id, message);
            }
        }

        // Missing top-level tasks with complete bytes are added first, so
        // their subtasks' rows and references to them resolve below.
        let mut taken = existing.keys().copied().collect::<HashSet<_>>();
        let mut next_root = (0..self.container.packs.len()).map(|pack| self.root_count(pack)).collect::<Result<Vec<_>, _>>()?;
        let mut working = BTreeMap::<(usize, usize), (Option<Vec<u8>>, Node)>::new();
        let mut added_positions = HashMap::<u32, (usize, usize, Vec<usize>)>::new();
        let mut highest_added = 0;
        for (index, row) in rows.iter().enumerate() {
            let id = match ids[index] { Some(id) if valid[index] && !existing.contains_key(&id) => id, _ => continue };
            let Some(raw) = row.get("_raw").and_then(Json::as_str) else { continue };
            if !allow_additions {
                valid[index] = false;
                reject(&mut report, index + 1, Some(id), "Whole tasks can be added only between identical task versions and layouts".into());
                continue;
            }
            let attempt = (|| -> Result<(usize, usize, Node, Vec<(u32, Vec<usize>)>), String> {
                let bytes = unhex(raw)?;
                verify_task_root(&self.schema, &bytes, version, "_raw is not a valid task of this version")?;
                let node = decode_exact(&self.schema, &bytes, version)?;
                let (raw_id, _) = task_heading(&node)?;
                if raw_id != id {
                    return Err(format!("_raw holds task {raw_id}, not {id}"));
                }
                let mut positions = Vec::new();
                tree_positions(&node, &mut Vec::new(), &mut positions)?;
                let mut tree_ids = Vec::new();
                collect_task_ids(&node, &mut tree_ids)?;
                let unique = tree_ids.iter().collect::<HashSet<_>>();
                if unique.len() != tree_ids.len() {
                    return Err("The task tree in _raw uses an ID more than once".into());
                }
                if let Some(clash) = tree_ids.iter().find(|tree_id| taken.contains(tree_id)) {
                    return Err(format!("Its subtask ID {clash} is already used in the open set (or by another added task)"));
                }
                let preferred = row.get("_pack").and_then(Json::as_u64).map(|pack| pack as usize).filter(|pack| next_root.get(*pack).is_some_and(|count| *count < ROOTS_PER_PACK));
                let pack = preferred.or_else(|| next_root.iter().position(|count| *count < ROOTS_PER_PACK))
                    .ok_or_else(|| format!("Every task pack already holds {ROOTS_PER_PACK} top-level tasks"))?;
                Ok((pack, next_root[pack], node, positions))
            })();
            match attempt {
                Ok((pack, root, node, positions)) => {
                    next_root[pack] += 1;
                    let (task_id, name) = task_heading(&node)?;
                    for (tree_id, path) in &positions {
                        taken.insert(*tree_id);
                        highest_added = highest_added.max(*tree_id);
                        added_positions.insert(*tree_id, (pack, root, path.clone()));
                    }
                    report.adding += 1;
                    if report.additions.len() < PREVIEW_ROWS {
                        report.additions.push(ImportAddition { source_row: index + 1, id: task_id, name, pack, tasks: positions.len() });
                    }
                    working.insert((pack, root), (None, node));
                }
                Err(message) => {
                    valid[index] = false;
                    reject(&mut report, index + 1, Some(id), format!("Cannot add: {message}"));
                }
            }
        }

        // Field updates, each row all or nothing.
        let known = |id: u32| taken.contains(&id);
        for (index, row) in rows.iter().enumerate() {
            let Some(id) = ids[index].filter(|_| valid[index]) else { continue };
            let Some((pack, root, path)) = existing.get(&id).and_then(|places| places.first().cloned()).or_else(|| added_positions.get(&id).cloned()) else {
                reject(&mut report, index + 1, Some(id), "No task has this ID. Only top-level tasks exported with _raw from the same task version can be added.".into());
                continue;
            };
            let fields = match row.get("fields") {
                None => Map::new(),
                Some(Json::Object(fields)) => fields.clone(),
                Some(_) => {
                    reject(&mut report, index + 1, Some(id), "\"fields\" must be an object of field paths and values".into());
                    continue;
                }
            };
            if !working.contains_key(&(pack, root)) {
                let bytes = self.current_root(pack, root)?;
                let node = decode_exact(&self.schema, &bytes, version)?;
                working.insert((pack, root), (Some(bytes), node));
            }
            let mut candidate = working[&(pack, root)].1.clone();
            let outcome = (|| -> Result<(String, Vec<(String, String, String)>), String> {
                let task = task_at_mut(&mut candidate, &path)?;
                let name = task_heading(task)?.1;
                let mut changed = Vec::new();
                for (key, value) in &fields {
                    let parts = field_path(key)?;
                    let text = input_text(value).map_err(|error| format!("{key}: {error}"))?;
                    if let Some((old, new)) = set_task_field(&self.schema, task, &parts, &text, &known).map_err(|error| format!("{key}: {error}"))? {
                        changed.push((label(&parts), old, new));
                    }
                }
                Ok((name, changed))
            })();
            match outcome {
                Ok((_, changed)) if changed.is_empty() => {
                    if !added_positions.contains_key(&id) {
                        report.unchanged += 1;
                    }
                }
                Ok((name, changed)) => {
                    if !added_positions.contains_key(&id) {
                        report.changing += 1;
                    }
                    report.fields += changed.len();
                    for (field, old, new) in changed {
                        if report.changes.len() < PREVIEW_ROWS {
                            report.changes.push(ImportChange { source_row: index + 1, id, name: name.clone(), field, old, new });
                        }
                    }
                    working.get_mut(&(pack, root)).unwrap().1 = candidate;
                }
                Err(message) => reject(&mut report, index + 1, Some(id), message),
            }
        }

        // Root changes: edited roots, then appended roots in pack order.
        let mut changes = Vec::new();
        let mut appended = Vec::new();
        for ((pack, root), (before, node)) in working {
            let after = node.encode()?;
            verify_task_root(&self.schema, &after, version, "The imported task would be invalid")?;
            match before {
                Some(before) if before != after => changes.push(RootChange { pack, root, before, after }),
                Some(_) => {}
                None => appended.push(RootChange { pack, root, before: Vec::new(), after }),
            }
        }
        changes.extend(appended);
        Ok(Plan { report, changes, highest_added })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_index(document: &TaskDocument) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !document.search("", 0).indexed {
            assert!(std::time::Instant::now() < deadline, "background task index timed out");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn rewrite(path: &Path, change: impl FnOnce(&mut Json)) {
        let mut document: Json = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        change(&mut document);
        std::fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
    }

    #[test]
    fn exports_updates_adds_and_undoes_real_tasks() {
        let source = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(source).is_file() {
            return;
        }
        let mut document = TaskDocument::open(source).unwrap();
        wait_for_index(&document);
        let folder = std::env::temp_dir().join(format!("jdide-task-json-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("tasks.json");
        let parent = document.summary().roots.into_iter().find(|root| root.child_count > 0).unwrap();
        let original = document.current_root(parent.pack, parent.root).unwrap();

        // Export a tree: the root carries _raw, subtasks carry fields only.
        let report = document.export_json(&[ExportTarget { pack: parent.pack, root: parent.root, path: Vec::new() }], true, &file).unwrap();
        assert!(report.tasks > 1);
        let exported: Json = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let rows = exported["tasks"].as_array().unwrap();
        assert!(rows[0]["_raw"].as_str().is_some() && rows[1].get("_raw").is_none());
        assert_eq!(rows[0]["fields"]["fixed.name"].as_str().unwrap(), parent.name);
        assert!(rows[0]["fields"].get("fixed.id").is_none() && rows[0]["fields"].get("fixed.hierarchy_parent").is_none());

        // An unchanged export previews as unchanged.
        let preview = document.import_json(&file, None).unwrap();
        assert_eq!((preview.changing, preview.adding, preview.rejected), (0, 0, 0));
        assert_eq!(preview.unchanged, report.tasks);

        // A renamed task updates; a locked field and a broken reference reject their rows.
        rewrite(&file, |document| {
            let rows = document["tasks"].as_array_mut().unwrap();
            rows[0]["fields"]["fixed.name"] = json!("JD IDE JSON test");
            rows[1]["fields"]["fixed.id"] = json!(1);
            if rows.len() > 2 {
                rows[2]["fields"]["fixed.premise_tasks[0]"] = json!(4_000_000_000u32);
            }
        });
        let preview = document.import_json(&file, None).unwrap();
        assert_eq!(preview.changing, 1);
        assert_eq!(preview.changes[0].new, "JD IDE JSON test");
        assert!(preview.issues.iter().any(|issue| issue.message.contains("fixed.id")));
        if report.tasks > 2 {
            assert!(preview.issues.iter().any(|issue| issue.message.contains("does not exist")));
        }
        // A stale preview cannot be applied.
        let stale = preview.token.clone();
        document.edit_field(super::super::browser::FieldEdit { pack: parent.pack, root: parent.root, task_path: Vec::new(), field_path: vec!["fixed".into(), "time_limit".into()], value: "77".into() }).unwrap();
        assert!(document.import_json(&file, Some(&stale)).is_err());
        document.undo().unwrap();
        let preview = document.import_json(&file, None).unwrap();
        let applied = document.import_json(&file, Some(&preview.token)).unwrap();
        assert!(applied.state.is_some());
        assert_eq!(document.task(parent.pack, parent.root, &[]).unwrap().name, "JD IDE JSON test");
        document.undo().unwrap();
        assert_eq!(document.current_root(parent.pack, parent.root).unwrap(), original);

        // A missing top-level tree is added from _raw with its IDs: export a
        // clone, take the clone back, then import it.
        let clone = document.clone_root_task(parent.pack, parent.root).unwrap();
        document.export_json(&[ExportTarget { pack: clone.pack, root: clone.root, path: Vec::new() }], true, &file).unwrap();
        let cloned_bytes = document.current_root(clone.pack, clone.root).unwrap();
        document.undo().unwrap();
        let roots = document.summary().root_count;
        let preview = document.import_json(&file, None).unwrap();
        assert_eq!((preview.adding, preview.rejected), (1, 0), "{:?}", preview.issues);
        assert_eq!(preview.additions[0].id, clone.id);
        document.import_json(&file, Some(&preview.token)).unwrap();
        assert_eq!(document.summary().root_count, roots + 1);
        let added = document.summary().roots.into_iter().find(|root| root.id == clone.id).unwrap();
        assert_eq!(document.current_root(added.pack, added.root).unwrap(), cloned_bytes);
        // Importing it again finds the tree present and changes nothing.
        let again = document.import_json(&file, None).unwrap();
        assert_eq!((again.adding, again.changing, again.rejected), (0, 0, 0));
        document.undo().unwrap();
        assert_eq!(document.summary().root_count, roots);

        // Another version is refused before anything is read.
        rewrite(&file, |document| document["taskVersion"] = json!(172));
        assert!(document.import_json(&file, None).unwrap_err().contains("v172"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn field_paths_round_trip() {
        for text in ["fixed.name", "fixed.premise_tasks[0]", "award.items[2].item_id", "dialogs.windows[1][3]"] {
            assert_eq!(dotted(&field_path(text).unwrap()), text);
        }
        assert!(field_path("fixed..name").is_err());
        assert!(field_path("fixed.items[x]").is_err());
        assert!(field_path("fixed.items[1").is_err());
    }

    #[test]
    fn floats_export_in_their_shortest_exact_form() {
        let schema = Schema { root: "T".into(), structs: BTreeMap::from([("T".to_string(), super::super::schema::StructDef { fields: vec![super::super::schema::FieldDef::new("rate", FieldType::F32)] })]) };
        let decoded = decode_exact(&schema, &0.1f32.to_le_bytes(), 0).unwrap();
        let value = json_value(decoded.child("rate").unwrap()).unwrap();
        assert_eq!(value.to_string(), "0.1");
        assert_eq!(input_text(&value).unwrap().parse::<f32>().unwrap(), 0.1f32);
    }
}
