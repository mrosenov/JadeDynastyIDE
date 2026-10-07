//! Read-only view models for the tasks.data browser.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

use super::container::{Pack, TaskContainer};
use super::schema::{decode_exact, probe_root_integer_validated, FieldType, Node, Schema, Value};
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

pub struct TaskDocument {
    container: TaskContainer,
    schema: Schema,
    summary: FileSummary,
    cache: Option<CachedRoot>,
}

struct CachedRoot {
    pack: usize,
    root: usize,
    bytes: usize,
    node: Node,
}

impl TaskDocument {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let container = TaskContainer::open(path)?;
        let schema = schema_for_version(container.header.version)?;
        let roots = read_root_summaries(&container, &schema)?;
        let summary = FileSummary {
            path: container.index_path().display().to_string(),
            version: container.header.version,
            export_version: container.header.export_version,
            root_count: roots.len(),
            pack_count: container.packs.len(),
            size: std::fs::metadata(container.index_path()).map(|metadata| metadata.len()).unwrap_or(0)
                + container.total_pack_bytes(),
            roots,
        };
        Ok(Self { container, schema, summary, cache: None })
    }

    pub fn summary(&self) -> FileSummary {
        self.summary.clone()
    }

    pub fn task(&mut self, pack: usize, root: usize, path: &[usize]) -> Result<TaskDetail, String> {
        let reload = self.cache.as_ref().map_or(true, |cached| cached.pack != pack || cached.root != root);
        if reload {
            let bytes = self.container.root(pack, root)?;
            let node = decode_exact(&self.schema, &bytes, self.container.header.version)?;
            self.cache = Some(CachedRoot { pack, root, bytes: bytes.len(), node });
        }
        let cached = self.cache.as_ref().expect("cache was populated");
        let tree = tree_view(&cached.node, Vec::new())?;
        let selected = task_at(&cached.node, path)?;
        let (id, name) = task_heading(selected)?;
        let fields = selected
            .children()
            .iter()
            .filter(|field| field.name != "subtasks")
            .map(field_view)
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

fn field_view(node: &Node) -> FieldView {
    let children = node.children().iter().map(field_view).collect::<Vec<_>>();
    let raw = matches!(node.ty, FieldType::Raw { .. });
    let (value, interpretation) = match &node.value {
        Value::I64(value) => (Some(value.to_string()), None),
        Value::U64(value) => (Some(value.to_string()), None),
        Value::F32(value) => (Some(format_float(*value as f64)), None),
        Value::F64(value) => (Some(format_float(*value)), None),
        Value::Bool(value) => (Some(value.to_string()), None),
        Value::Text(value) => (Some(value.clone()), None),
        Value::Bytes(bytes) => (Some(hex_preview(bytes)), raw_interpretation(bytes)),
        Value::Struct(values) => (Some(format!("{} fields", values.len())), None),
        Value::Array(values) => (Some(format!("{} items", values.len())), None),
    };
    FieldView {
        name: node.name.clone(),
        offset: node.offset,
        size: node.byte_len,
        ty: type_name(&node.ty),
        value,
        interpretation,
        children,
        raw,
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
        }
    }
}
