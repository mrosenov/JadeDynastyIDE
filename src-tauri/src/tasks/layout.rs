//! User-owned patches for task versions that do not yet have a built-in layout.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::schema::{FieldDef, FieldType, PatchOperation, Schema};
use super::{schema_for_version, supported_versions};

const FORMAT_VERSION: u32 = 1;
const FOLDER: &str = "task-layouts";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTaskLayout {
    pub format_version: u32,
    pub task_version: u32,
    pub base_version: u32,
    pub operations: Vec<PatchOperation>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationView {
    pub index: usize,
    pub kind: String,
    pub structure: String,
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSummary {
    pub path: String,
    pub task_version: u32,
    pub base_version: u32,
    pub operations: Vec<OperationView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutReport {
    pub patch: LayoutSummary,
    pub analysis: super::analyze::AnalysisReport,
}

impl UserTaskLayout {
    pub fn new(task_version: u32, base_version: u32) -> Result<Self, String> {
        if supported_versions().contains(&task_version) {
            return Err(format!("tasks.data v{task_version} already has a verified built-in layout"));
        }
        let layout = Self { format_version: FORMAT_VERSION, task_version, base_version, operations: Vec::new() };
        layout.validate()?;
        Ok(layout)
    }

    pub fn validate(&self) -> Result<Schema, String> {
        if self.format_version != FORMAT_VERSION {
            return Err(format!("unsupported task-layout format {}; expected {FORMAT_VERSION}", self.format_version));
        }
        if !supported_versions().contains(&self.base_version) {
            return Err(format!("v{} is not a verified task-layout baseline", self.base_version));
        }
        if self.base_version > self.task_version {
            return Err(format!("base v{} is newer than target v{}", self.base_version, self.task_version));
        }
        schema_for_version(self.base_version)?.with_operations(&self.operations)
    }

    pub fn add_fixed(&mut self, structure: String, after_field: String, name: String, width: usize, field_type: &str) -> Result<Schema, String> {
        validate_field_name(&name)?;
        if !(1..=4096).contains(&width) {
            return Err("Fixed task fields must contain between 1 and 4096 bytes".into());
        }
        let ty = parse_fixed_type(field_type)?;
        let type_width = fixed_type_width(&ty).ok_or_else(|| format!("{field_type:?} is not a fixed-width task field type"))?;
        if type_width != width {
            return Err(format!("{field_type} occupies {type_width} byte{}, but this candidate occupies {width} byte{}", if type_width == 1 { "" } else { "s" }, if width == 1 { "" } else { "s" }));
        }
        self.operations.push(PatchOperation::InsertAfter {
            structure,
            after: Some(after_field),
            field: FieldDef::new(name, ty),
        });
        match self.validate() {
            Ok(schema) => Ok(schema),
            Err(error) => {
                self.operations.pop();
                Err(error)
            }
        }
    }

    pub fn add_raw(&mut self, structure: String, after_field: String, name: String, width: usize) -> Result<Schema, String> {
        self.add_fixed(structure, after_field, name, width, &raw_type_name(width))
    }

    pub fn set_type(&mut self, index: usize, field_type: &str) -> Result<Schema, String> {
        let operation = self.operations.get_mut(index).ok_or_else(|| format!("Task-layout operation {} does not exist", index + 1))?;
        let PatchOperation::InsertAfter { structure, field, .. } = operation else {
            return Err("Only inserted task fields can change type".into());
        };
        let old_width = fixed_type_width(&field.ty).ok_or_else(|| format!("{structure}.{} is not a fixed-width task field", field.name))?;
        let ty = parse_fixed_type(field_type)?;
        let new_width = fixed_type_width(&ty).ok_or_else(|| format!("{field_type:?} is not a fixed-width task field type"))?;
        if new_width != old_width {
            return Err(format!("Changing this field from {old_width} to {new_width} bytes would move every following field"));
        }
        let previous = std::mem::replace(&mut field.ty, ty);
        match self.validate() {
            Ok(schema) => Ok(schema),
            Err(error) => {
                if let PatchOperation::InsertAfter { field, .. } = &mut self.operations[index] {
                    field.ty = previous;
                }
                Err(error)
            }
        }
    }

    pub fn remove(&mut self, index: usize) -> Result<Schema, String> {
        if index >= self.operations.len() {
            return Err(format!("Task-layout operation {} does not exist", index + 1));
        }
        let operation = self.operations.remove(index);
        match self.validate() {
            Ok(schema) => Ok(schema),
            Err(error) => {
                self.operations.insert(index, operation);
                Err(error)
            }
        }
    }
}

pub fn path(root: &Path, version: u32) -> PathBuf {
    root.join(FOLDER).join(format!("v{version}.json"))
}

pub fn load(root: &Path, version: u32) -> Result<Option<UserTaskLayout>, String> {
    let path = path(root, version);
    if !path.is_file() { return Ok(None); }
    let bytes = std::fs::read(&path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let layout: UserTaskLayout = serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if layout.task_version != version {
        return Err(format!("{} describes v{}, expected v{version}", path.display(), layout.task_version));
    }
    layout.validate()?;
    Ok(Some(layout))
}

pub fn save(root: &Path, layout: &UserTaskLayout) -> Result<(), String> {
    layout.validate()?;
    let path = path(root, layout.task_version);
    if layout.operations.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| format!("Could not remove {}: {error}", path.display()))?;
        }
        return Ok(());
    }
    let folder = path.parent().unwrap();
    std::fs::create_dir_all(folder).map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
    let json = serde_json::to_string_pretty(layout).map_err(|error| error.to_string())? + "\n";
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, json).map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, &path).map_err(|error| format!("Could not write {}: {error}", path.display()))
}

pub fn summary(root: &Path, layout: &UserTaskLayout) -> LayoutSummary {
    let operations = layout.operations.iter().enumerate().map(|(index, operation)| match operation {
        PatchOperation::InsertAfter { structure, after, field } => OperationView {
            index,
            kind: "insert".into(),
            structure: structure.clone(),
            field: field.name.clone(),
            after_field: after.clone(),
            width: fixed_type_width(&field.ty),
            field_type: fixed_type_name(&field.ty),
        },
        PatchOperation::Remove { structure, field } => OperationView {
            index,
            kind: "remove".into(),
            structure: structure.clone(),
            field: field.clone(),
            after_field: None,
            width: None,
            field_type: None,
        },
        PatchOperation::Replace { structure, field: _, replacement } => OperationView {
            index,
            kind: "replace".into(),
            structure: structure.clone(),
            field: replacement.name.clone(),
            after_field: None,
            width: fixed_type_width(&replacement.ty),
            field_type: fixed_type_name(&replacement.ty),
        },
    }).collect();
    LayoutSummary { path: path(root, layout.task_version).display().to_string(), task_version: layout.task_version, base_version: layout.base_version, operations }
}

fn parse_fixed_type(name: &str) -> Result<FieldType, String> {
    let ty = match name.trim().to_ascii_lowercase().as_str() {
        "int8" => FieldType::I8,
        "uint8" => FieldType::U8,
        "bool8" => FieldType::Bool8,
        "int16" => FieldType::I16,
        "uint16" => FieldType::U16,
        "int32" => FieldType::I32,
        "uint32" => FieldType::U32,
        "int64" => FieldType::I64,
        "uint64" => FieldType::U64,
        "float32" => FieldType::F32,
        "float64" => FieldType::F64,
        value if value.starts_with("raw[") && value.ends_with(']') => FieldType::Raw { len: parse_bracketed_width(value, "raw")? },
        value if value.starts_with("bytes[") && value.ends_with(']') => FieldType::Bytes { len: parse_bracketed_width(value, "bytes")? },
        "raw8" => FieldType::Raw { len: 1 },
        "raw16" => FieldType::Raw { len: 2 },
        "raw32" => FieldType::Raw { len: 4 },
        "raw64" => FieldType::Raw { len: 8 },
        _ => return Err(format!("Unsupported fixed-width task field type {name:?}")),
    };
    let width = fixed_type_width(&ty).unwrap();
    if !(1..=4096).contains(&width) {
        return Err("Fixed task fields must contain between 1 and 4096 bytes".into());
    }
    Ok(ty)
}

fn parse_bracketed_width(value: &str, prefix: &str) -> Result<usize, String> {
    value[prefix.len() + 1..value.len() - 1].parse().map_err(|_| format!("Invalid task field type {value:?}"))
}

fn fixed_type_width(ty: &FieldType) -> Option<usize> {
    match ty {
        FieldType::I8 | FieldType::U8 | FieldType::Bool8 => Some(1),
        FieldType::I16 | FieldType::U16 => Some(2),
        FieldType::I32 | FieldType::U32 | FieldType::F32 => Some(4),
        FieldType::I64 | FieldType::U64 | FieldType::F64 => Some(8),
        FieldType::Raw { len } | FieldType::Bytes { len } => Some(*len),
        _ => None,
    }
}

fn fixed_type_name(ty: &FieldType) -> Option<String> {
    Some(match ty {
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
        FieldType::Raw { len } => raw_type_name(*len),
        FieldType::Bytes { len } => format!("bytes[{len}]"),
        _ => return None,
    })
}

fn raw_type_name(width: usize) -> String {
    match width {
        1 => "raw8".into(),
        2 => "raw16".into(),
        4 => "raw32".into(),
        8 => "raw64".into(),
        width => format!("raw[{width}]"),
    }
}

fn validate_field_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return Err("Field name is required".into()) };
    if !(first == '_' || first.is_ascii_alphabetic()) || !chars.all(|character| character == '_' || character.is_ascii_alphanumeric()) {
        return Err("Field names must use ASCII letters, numbers and underscores, and cannot start with a number".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_insertions_change_type_persist_and_remove() {
        let root = std::env::temp_dir().join(format!("jdide-task-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut layout = UserTaskLayout::new(203, 184).unwrap();
        layout.add_fixed("TASK_FIXED_V184".into(), "unknown_v184_0".into(), "unknown_v203_1".into(), 4, "raw32").unwrap();
        layout.set_type(0, "uint32").unwrap();
        assert!(layout.set_type(0, "uint16").unwrap_err().contains("move every following field"));
        assert_eq!(fixed_type_width(&parse_fixed_type("bytes[32]").unwrap()), Some(32));
        save(&root, &layout).unwrap();
        let loaded = load(&root, 203).unwrap().unwrap();
        assert_eq!(loaded, layout);
        let view = summary(&root, &loaded);
        assert_eq!(view.operations[0].width, Some(4));
        assert_eq!(view.operations[0].field_type.as_deref(), Some("uint32"));
        assert_eq!(view.operations[0].after_field.as_deref(), Some("unknown_v184_0"));
        layout.remove(0).unwrap();
        save(&root, &layout).unwrap();
        assert!(!path(&root, 203).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
