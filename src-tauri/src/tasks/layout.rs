//! User-owned patches for task versions that do not yet have a built-in layout.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::schema::{Condition, FieldDef, FieldType, PatchOperation, Predicate, Schema};
use super::{schema_for_version, supported_versions};

const FORMAT_VERSION: u32 = 1;
const FOLDER: &str = "task-layouts";
const MAX_LAYOUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserTaskLayout {
    pub format_version: u32,
    pub task_version: u32,
    pub base_version: u32,
    pub operations: Vec<PatchOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<LayoutVerification>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutVerification {
    pub layout_digest: String,
    pub verified_at: u64,
    pub root_count: usize,
    pub byte_count: u64,
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
    pub conditions: Vec<ConditionView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionView {
    pub field: String,
    pub operator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionInput {
    pub field: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSummary {
    pub path: String,
    pub task_version: u32,
    pub base_version: u32,
    pub operations: Vec<OperationView>,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_roots: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutReport {
    pub patch: LayoutSummary,
    pub analysis: super::analyze::AnalysisReport,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub path: String,
    pub task_version: u32,
    pub base_version: u32,
    pub operations: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaView {
    pub task_version: u32,
    pub baseline_version: u32,
    pub source: String,
    pub root: String,
    pub structures: Vec<StructureView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureView {
    pub name: String,
    pub root: bool,
    pub fields: Vec<FieldView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldView {
    pub name: String,
    pub field_type: String,
    pub conditions: Vec<String>,
    pub patched: bool,
    pub integer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed_width: Option<usize>,
}

impl UserTaskLayout {
    pub fn new(task_version: u32, base_version: u32) -> Result<Self, String> {
        if supported_versions().contains(&task_version) {
            return Err(format!("tasks.data v{task_version} already has a verified built-in layout"));
        }
        let layout = Self { format_version: FORMAT_VERSION, task_version, base_version, operations: Vec::new(), verification: None };
        layout.validate()?;
        Ok(layout)
    }

    fn digest(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(&(self.format_version, self.task_version, self.base_version, &self.operations)).map_err(|error| error.to_string())?;
        Ok(format!("{:x}", Md5::digest(bytes)))
    }

    pub fn is_verified(&self) -> bool {
        self.verification.as_ref().is_some_and(|verification| self.digest().is_ok_and(|digest| digest == verification.layout_digest))
    }

    pub fn mark_verified(&mut self, root_count: usize, byte_count: u64) -> Result<(), String> {
        self.verification = Some(LayoutVerification {
            layout_digest: self.digest()?,
            verified_at: SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_secs(),
            root_count,
            byte_count,
        });
        Ok(())
    }

    pub fn clear_verification(&mut self) {
        self.verification = None;
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
        for operation in &self.operations {
            let field = match operation {
                PatchOperation::InsertAfter { field, .. } => Some(field),
                PatchOperation::Replace { replacement, .. } => Some(replacement),
                PatchOperation::Remove { .. } => None,
            };
            let Some(field) = field else { continue };
            if field.when.len() > 16 {
                return Err(format!("Task field {:?} has more than 16 conditions", field.name));
            }
            if field.when.iter().any(|condition| matches!(condition, Condition::Version { .. })) {
                return Err(format!("Task field {:?} cannot use a version condition in a version-specific user layout", field.name));
            }
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
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
            Err(error) => {
                self.operations.pop();
                Err(error)
            }
        }
    }

    pub fn add_raw(&mut self, structure: String, after_field: String, name: String, width: usize) -> Result<Schema, String> {
        self.add_fixed(structure, after_field, name, width, &raw_type_name(width))
    }

    pub fn add_counted_array(&mut self, structure: String, after_field: String, name: String, count_field: String, item_type: String) -> Result<Schema, String> {
        validate_field_name(&name)?;
        let item = parse_array_item_type(&item_type)?;
        self.operations.push(PatchOperation::InsertAfter {
            structure,
            after: Some(after_field),
            field: FieldDef::new(name, FieldType::CountedArray { count_field: count_field.trim().into(), item: Box::new(item) }),
        });
        match self.validate() {
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
            Err(error) => {
                self.operations.pop();
                Err(error)
            }
        }
    }

    pub fn remove_field(&mut self, structure: String, field: String) -> Result<Schema, String> {
        self.ensure_baseline_field(&structure, &field)?;
        self.operations.push(PatchOperation::Remove { structure, field });
        match self.validate() {
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
            Err(error) => {
                self.operations.pop();
                Err(error)
            }
        }
    }

    pub fn replace_field_type(&mut self, structure: String, field: String, field_type: &str) -> Result<Schema, String> {
        self.ensure_baseline_field(&structure, &field)?;
        let schema = self.validate()?;
        let current = schema.structs.get(&structure).and_then(|definition| definition.fields.iter().find(|candidate| candidate.name == field))
            .ok_or_else(|| format!("Task layout cannot find {structure}.{field}"))?;
        if fixed_type_width(&current.ty).is_none() {
            return Err(format!("Only fixed-width baseline fields can change type; {structure}.{field} is {}", describe_type(&current.ty)));
        }
        let mut replacement = current.clone();
        replacement.ty = parse_fixed_type(field_type)?;
        self.operations.push(PatchOperation::Replace { structure, field, replacement });
        match self.validate() {
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
            Err(error) => {
                self.operations.pop();
                Err(error)
            }
        }
    }

    fn ensure_baseline_field(&self, structure: &str, field: &str) -> Result<(), String> {
        let baseline = schema_for_version(self.base_version)?;
        if !baseline.structs.get(structure).is_some_and(|definition| definition.fields.iter().any(|candidate| candidate.name == field)) {
            return Err(format!("{structure}.{field} is not an inherited baseline field"));
        }
        if self.operations.iter().any(|operation| match operation {
            PatchOperation::InsertAfter { structure: target, field: inserted, .. } => target == structure && inserted.name == field,
            PatchOperation::Remove { structure: target, field: removed } => target == structure && removed == field,
            PatchOperation::Replace { structure: target, field: replaced, .. } => target == structure && replaced == field,
        }) {
            return Err(format!("{structure}.{field} already has a user-layout operation"));
        }
        Ok(())
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
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
            Err(error) => {
                if let PatchOperation::InsertAfter { field, .. } = &mut self.operations[index] {
                    field.ty = previous;
                }
                Err(error)
            }
        }
    }

    pub fn set_conditions(&mut self, index: usize, inputs: Vec<ConditionInput>) -> Result<Schema, String> {
        if inputs.len() > 16 {
            return Err("A task field can have at most 16 conditions".into());
        }
        let conditions = inputs.iter().map(parse_condition).collect::<Result<Vec<_>, _>>()?;
        let operation = self.operations.get_mut(index).ok_or_else(|| format!("Task-layout operation {} does not exist", index + 1))?;
        let PatchOperation::InsertAfter { field, .. } = operation else {
            return Err("Only inserted task fields can change conditions".into());
        };
        let previous = std::mem::replace(&mut field.when, conditions);
        match self.validate() {
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
            Err(error) => {
                if let PatchOperation::InsertAfter { field, .. } = &mut self.operations[index] {
                    field.when = previous;
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
            Ok(schema) => {
                self.clear_verification();
                Ok(schema)
            }
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
    let layout = read(&path)?;
    if layout.task_version != version {
        return Err(format!("{} describes v{}, expected v{version}", path.display(), layout.task_version));
    }
    layout.validate()?;
    Ok(Some(layout))
}

pub fn save(root: &Path, layout: &UserTaskLayout) -> Result<(), String> {
    layout.validate()?;
    let path = path(root, layout.task_version);
    if layout.operations.is_empty() && !layout.is_verified() {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|error| format!("Could not remove {}: {error}", path.display()))?;
        }
        return Ok(());
    }
    write(&path, layout)
}

pub fn read(path: &Path) -> Result<UserTaskLayout, String> {
    let metadata = std::fs::metadata(path).map_err(|error| format!("Could not inspect {}: {error}", path.display()))?;
    if metadata.len() > MAX_LAYOUT_BYTES {
        return Err(format!("{} is too large to be a task-layout patch", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let layout: UserTaskLayout = serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    layout.validate()?;
    Ok(layout)
}

pub fn export(root: &Path, version: u32, target: &Path) -> Result<ExportReport, String> {
    let layout = load(root, version)?.ok_or_else(|| format!("No user task layout exists for v{version}"))?;
    write(target, &layout)?;
    Ok(ExportReport {
        path: target.display().to_string(),
        task_version: layout.task_version,
        base_version: layout.base_version,
        operations: layout.operations.len(),
    })
}

pub fn schema_view(root: &Path, version: u32, baseline_version: u32) -> Result<SchemaView, String> {
    let (baseline_version, source, schema, operations) = if supported_versions().contains(&version) {
        (version, "built_in".to_string(), schema_for_version(version)?, Vec::new())
    } else if let Some(layout) = load(root, version)? {
        (layout.base_version, "user_patch".to_string(), layout.validate()?, layout.operations)
    } else {
        if !supported_versions().contains(&baseline_version) {
            return Err(format!("v{baseline_version} is not a verified task-layout baseline"));
        }
        if baseline_version > version {
            return Err(format!("base v{baseline_version} is newer than target v{version}"));
        }
        (baseline_version, "baseline".to_string(), schema_for_version(baseline_version)?, Vec::new())
    };
    let patched = operations.iter().filter_map(|operation| match operation {
        PatchOperation::InsertAfter { structure, field, .. } => Some((structure.clone(), field.name.clone())),
        PatchOperation::Replace { structure, replacement, .. } => Some((structure.clone(), replacement.name.clone())),
        PatchOperation::Remove { .. } => None,
    }).collect::<HashSet<_>>();
    let structures = schema.structs.iter().map(|(name, definition)| StructureView {
        name: name.clone(),
        root: *name == schema.root,
        fields: definition.fields.iter().map(|field| FieldView {
            name: field.name.clone(),
            field_type: describe_type(&field.ty),
            conditions: field.when.iter().map(describe_condition).collect(),
            patched: patched.contains(&(name.clone(), field.name.clone())),
            integer: is_integer_type(&field.ty),
            fixed_width: fixed_type_width(&field.ty),
        }).collect(),
    }).collect();
    Ok(SchemaView { task_version: version, baseline_version, source, root: schema.root, structures })
}

fn write(path: &Path, layout: &UserTaskLayout) -> Result<(), String> {
    layout.validate()?;
    let folder = path.parent().ok_or_else(|| format!("{} has no parent folder", path.display()))?;
    std::fs::create_dir_all(folder).map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
    let json = serde_json::to_string_pretty(layout).map_err(|error| error.to_string())? + "\n";
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, json).map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("Could not write {}: {error}", path.display())
    })
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
            field_type: Some(fixed_type_name(&field.ty).unwrap_or_else(|| describe_type(&field.ty))),
            conditions: condition_views(&field.when),
        },
        PatchOperation::Remove { structure, field } => OperationView {
            index,
            kind: "remove".into(),
            structure: structure.clone(),
            field: field.clone(),
            after_field: None,
            width: None,
            field_type: None,
            conditions: Vec::new(),
        },
        PatchOperation::Replace { structure, field: _, replacement } => OperationView {
            index,
            kind: "replace".into(),
            structure: structure.clone(),
            field: replacement.name.clone(),
            after_field: None,
            width: fixed_type_width(&replacement.ty),
            field_type: Some(fixed_type_name(&replacement.ty).unwrap_or_else(|| describe_type(&replacement.ty))),
            conditions: condition_views(&replacement.when),
        },
    }).collect();
    let verification = layout.verification.as_ref().filter(|_| layout.is_verified());
    LayoutSummary {
        path: path(root, layout.task_version).display().to_string(),
        task_version: layout.task_version,
        base_version: layout.base_version,
        operations,
        verified: verification.is_some(),
        verified_at: verification.map(|value| value.verified_at),
        verified_roots: verification.map(|value| value.root_count),
        verified_bytes: verification.map(|value| value.byte_count),
    }
}

fn parse_condition(input: &ConditionInput) -> Result<Condition, String> {
    let field = input.field.trim();
    if field.is_empty() {
        return Err("A condition needs a controller field".into());
    }
    let value = input.value.as_deref().unwrap_or("").trim();
    let integer = |label: &str| parse_integer(value).map_err(|_| format!("Condition {label} needs a decimal or hexadecimal integer"));
    let predicate = match input.operator.trim().to_ascii_lowercase().as_str() {
        "zero" => Predicate::Zero,
        "non_zero" => Predicate::NonZero,
        "eq" => Predicate::Eq(integer("=")?),
        "not_eq" => Predicate::NotEq(integer("≠")?),
        "at_least" => Predicate::AtLeast(integer("≥")?),
        "at_most" => Predicate::AtMost(integer("≤")?),
        "bits_any" => Predicate::BitsAny(parse_unsigned(value).map_err(|_| "Condition 'has any bits' needs a non-negative decimal or hexadecimal mask".to_string())?),
        "bits_all" => Predicate::BitsAll(parse_unsigned(value).map_err(|_| "Condition 'has all bits' needs a non-negative decimal or hexadecimal mask".to_string())?),
        "one_of" => {
            let values = value.split(',').map(str::trim).filter(|value| !value.is_empty()).map(parse_integer).collect::<Result<Vec<_>, _>>()
                .map_err(|_| "Condition 'one of' needs comma-separated decimal or hexadecimal integers".to_string())?;
            if values.is_empty() { return Err("Condition 'one of' needs at least one value".into()); }
            Predicate::OneOf(values)
        }
        operator => return Err(format!("Unsupported task condition operator {operator:?}")),
    };
    Ok(Condition::Field { field: field.into(), predicate })
}

fn parse_integer(value: &str) -> Result<i128, ()> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("-0x").or_else(|| value.strip_prefix("-0X")) {
        i128::from_str_radix(hex, 16).map(|number| -number).map_err(|_| ())
    } else if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        i128::from_str_radix(hex, 16).map_err(|_| ())
    } else {
        value.parse().map_err(|_| ())
    }
}

fn parse_unsigned(value: &str) -> Result<u128, ()> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        u128::from_str_radix(hex, 16).map_err(|_| ())
    } else {
        value.parse().map_err(|_| ())
    }
}

fn condition_views(conditions: &[Condition]) -> Vec<ConditionView> {
    conditions.iter().filter_map(|condition| match condition {
        Condition::Field { field, predicate } => {
            let (operator, value) = predicate_parts(predicate);
            Some(ConditionView { field: field.clone(), operator: operator.into(), value, label: describe_condition(condition) })
        }
        Condition::Version { .. } => None,
    }).collect()
}

fn predicate_parts(predicate: &Predicate) -> (&'static str, Option<String>) {
    match predicate {
        Predicate::Eq(value) => ("eq", Some(value.to_string())),
        Predicate::NotEq(value) => ("not_eq", Some(value.to_string())),
        Predicate::OneOf(values) => ("one_of", Some(values.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))),
        Predicate::Zero => ("zero", None),
        Predicate::NonZero => ("non_zero", None),
        Predicate::AtLeast(value) => ("at_least", Some(value.to_string())),
        Predicate::AtMost(value) => ("at_most", Some(value.to_string())),
        Predicate::BitsAny(mask) => ("bits_any", Some(format!("0x{mask:X}"))),
        Predicate::BitsAll(mask) => ("bits_all", Some(format!("0x{mask:X}"))),
    }
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

fn parse_array_item_type(name: &str) -> Result<FieldType, String> {
    if let Some(structure) = name.trim().strip_prefix("struct:") {
        let structure = structure.trim();
        if structure.is_empty() { return Err("A counted-array structure name is required".into()); }
        return Ok(FieldType::Named { name: structure.into() });
    }
    let ty = parse_fixed_type(name)?;
    if matches!(ty, FieldType::Raw { .. } | FieldType::Bytes { .. }) {
        return Err("Counted arrays need a scalar item type or an existing structure".into());
    }
    Ok(ty)
}

fn is_integer_type(ty: &FieldType) -> bool {
    matches!(ty, FieldType::I8 | FieldType::U8 | FieldType::Bool8 | FieldType::I16 | FieldType::U16 | FieldType::I32 | FieldType::U32 | FieldType::I64 | FieldType::U64)
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

fn describe_type(ty: &FieldType) -> String {
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
        FieldType::PrefixedUtf16 { prefix, .. } => format!("wstring<{prefix:?} length>"),
        FieldType::CountedUtf16 { count_field, .. } => format!("wstring<{count_field}>"),
        FieldType::Bytes { len } => format!("bytes[{len}]"),
        FieldType::CountedBytes { count_field } => format!("bytes<{count_field}>"),
        FieldType::Raw { len } => raw_type_name(*len),
        FieldType::Named { name } => name.clone(),
        FieldType::FixedArray { len, item } => format!("{len} × {}", describe_type(item)),
        FieldType::CountedArray { count_field, item } => format!("{count_field} × {}", describe_type(item)),
        FieldType::RecursiveArray { count_field, target } => format!("{count_field} × {target}"),
    }
}

fn describe_condition(condition: &Condition) -> String {
    match condition {
        Condition::Version { min, max } => match (min, max) {
            (Some(min), Some(max)) => format!("v{min}–v{max}"),
            (Some(min), None) => format!("v{min}+"),
            (None, Some(max)) => format!("through v{max}"),
            (None, None) => "all versions".into(),
        },
        Condition::Field { field, predicate } => format!("{field} {}", describe_predicate(predicate)),
    }
}

fn describe_predicate(predicate: &Predicate) -> String {
    match predicate {
        Predicate::Eq(value) => format!("= {value}"),
        Predicate::NotEq(value) => format!("≠ {value}"),
        Predicate::OneOf(values) => format!("in [{}]", values.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")),
        Predicate::Zero => "= 0".into(),
        Predicate::NonZero => "≠ 0".into(),
        Predicate::AtLeast(value) => format!("≥ {value}"),
        Predicate::AtMost(value) => format!("≤ {value}"),
        Predicate::BitsAny(mask) => format!("has any 0x{mask:X}"),
        Predicate::BitsAll(mask) => format!("has all 0x{mask:X}"),
    }
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
    fn accepted_layouts_persist_and_schema_changes_clear_acceptance() {
        let root = std::env::temp_dir().join(format!("jdide-task-layout-verified-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut layout = UserTaskLayout::new(203, 184).unwrap();
        layout.mark_verified(37, 12_345).unwrap();
        assert!(layout.is_verified());
        save(&root, &layout).unwrap();
        assert!(path(&root, 203).is_file());
        let loaded = load(&root, 203).unwrap().unwrap();
        assert!(loaded.is_verified());
        let view = summary(&root, &loaded);
        assert!(view.verified);
        assert_eq!(view.verified_roots, Some(37));
        assert_eq!(view.verified_bytes, Some(12_345));

        layout.add_fixed("TASK_FIXED_V184".into(), "unknown_v184_0".into(), "unknown_v203_1".into(), 4, "raw32").unwrap();
        assert!(!layout.is_verified());
        assert!(layout.verification.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fixed_insertions_change_type_persist_and_remove() {
        let root = std::env::temp_dir().join(format!("jdide-task-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut layout = UserTaskLayout::new(203, 184).unwrap();
        layout.add_fixed("TASK_FIXED_V184".into(), "unknown_v184_0".into(), "unknown_v203_1".into(), 4, "raw32").unwrap();
        save(&root, &layout).unwrap();
        layout.set_type(0, "uint32").unwrap();
        assert!(layout.set_type(0, "uint16").unwrap_err().contains("move every following field"));
        layout.set_conditions(0, vec![ConditionInput { field: "id".into(), operator: "non_zero".into(), value: None }]).unwrap();
        assert!(layout.set_conditions(0, vec![ConditionInput { field: "unknown_v203_1".into(), operator: "eq".into(), value: Some("0x10".into()) }]).unwrap_err().contains("before it is read"));
        assert_eq!(fixed_type_width(&parse_fixed_type("bytes[32]").unwrap()), Some(32));
        save(&root, &layout).unwrap();
        let loaded = load(&root, 203).unwrap().unwrap();
        assert_eq!(loaded, layout);
        let view = summary(&root, &loaded);
        assert_eq!(view.operations[0].width, Some(4));
        assert_eq!(view.operations[0].field_type.as_deref(), Some("uint32"));
        assert_eq!(view.operations[0].after_field.as_deref(), Some("unknown_v184_0"));
        assert_eq!(view.operations[0].conditions[0].label, "id ≠ 0");
        let exported = root.join("shared-v203.json");
        let report = export(&root, 203, &exported).unwrap();
        assert_eq!(report.operations, 1);
        assert_eq!(read(&exported).unwrap(), layout);
        let schema = schema_view(&root, 203, 165).unwrap();
        assert_eq!(schema.source, "user_patch");
        assert_eq!(schema.baseline_version, 184);
        assert!(schema.structures.iter().flat_map(|structure| &structure.fields).any(|field| field.name == "unknown_v203_1" && field.patched && field.field_type == "uint32"));
        let built_in = schema_view(&root, 165, 165).unwrap();
        assert_eq!(built_in.source, "built_in");
        assert_eq!(built_in.baseline_version, 165);
        layout.remove(0).unwrap();
        save(&root, &layout).unwrap();
        assert!(!path(&root, 203).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn counted_arrays_require_an_earlier_integer_and_existing_item_structure() {
        let mut layout = UserTaskLayout::new(203, 184).unwrap();
        let schema = layout.add_counted_array("TASK_FIXED_V184".into(), "id".into(), "new_values".into(), "id".into(), "uint32".into()).unwrap();
        let field = schema.structs["TASK_FIXED_V184"].fields.iter().find(|field| field.name == "new_values").unwrap();
        assert_eq!(field.ty, FieldType::CountedArray { count_field: "id".into(), item: Box::new(FieldType::U32) });
        let view = summary(Path::new("unused"), &layout);
        assert_eq!(view.operations[0].field_type.as_deref(), Some("id × uint32"));

        let mut bad_count = UserTaskLayout::new(203, 184).unwrap();
        assert!(bad_count.add_counted_array("TASK_FIXED_V184".into(), "name".into(), "new_values".into(), "name".into(), "uint32".into()).unwrap_err().contains("not an integer"));
        let mut bad_item = UserTaskLayout::new(203, 184).unwrap();
        assert!(bad_item.add_counted_array("TASK_FIXED_V184".into(), "id".into(), "new_values".into(), "id".into(), "struct:MISSING".into()).unwrap_err().contains("unknown structure"));
    }

    #[test]
    fn inherited_fixed_fields_can_be_removed_or_retyped_once() {
        let mut removed = UserTaskLayout::new(203, 184).unwrap();
        let schema = removed.remove_field("TASK_FIXED_V184".into(), "unknown_v184_0".into()).unwrap();
        assert!(!schema.structs["TASK_FIXED_V184"].fields.iter().any(|field| field.name == "unknown_v184_0"));
        assert!(removed.remove_field("TASK_FIXED_V184".into(), "unknown_v184_0".into()).unwrap_err().contains("already has"));

        let mut replaced = UserTaskLayout::new(203, 184).unwrap();
        let schema = replaced.replace_field_type("TASK_FIXED_V184".into(), "id".into(), "uint64").unwrap();
        assert_eq!(schema.structs["TASK_FIXED_V184"].fields.iter().find(|field| field.name == "id").unwrap().ty, FieldType::U64);
        let view = summary(Path::new("unused"), &replaced);
        assert_eq!(view.operations[0].kind, "replace");
        assert_eq!(view.operations[0].field_type.as_deref(), Some("uint64"));
    }
}
