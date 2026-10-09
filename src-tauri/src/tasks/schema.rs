//! Declarative, versioned binary layouts for task records.
//!
//! Task formats change by inserting fields and conditional sections between
//! existing fields. This module keeps that binary shape separate from the
//! eventual UI labels and supplies a byte-preserving decoder. Unknown values
//! can stay typed as fixed-width raw fields until their meaning is known.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountWidth {
    U8,
    U16,
    U32,
}

impl CountWidth {
    fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 => 4,
        }
    }

    fn maximum(self) -> usize {
        match self {
            Self::U8 => u8::MAX as usize,
            Self::U16 => u16::MAX as usize,
            Self::U32 => u32::MAX as usize,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextLengthUnit {
    Utf16Units,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldType {
    I8,
    U8,
    Bool8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    FixedUtf16 { units: usize },
    PrefixedUtf16 {
        prefix: CountWidth,
        unit: TextLengthUnit,
        #[serde(default)]
        terminated: bool,
    },
    CountedUtf16 { count_field: String, unit: TextLengthUnit },
    Bytes { len: usize },
    CountedBytes { count_field: String },
    /// An unknown fixed-width value. The inspector may show its bytes plus
    /// integer and floating-point interpretations without assigning meaning.
    Raw { len: usize },
    Named { name: String },
    FixedArray { len: usize, item: Box<FieldType> },
    CountedArray {
        count_field: String,
        item: Box<FieldType>,
    },
    /// A counted array of task records. This is separate from `CountedArray`
    /// so recursive references are explicit and easy to constrain.
    RecursiveArray { count_field: String, target: String },
}

impl FieldType {
    fn is_numeric_scalar(&self) -> bool {
        matches!(
            self,
            Self::I8
                | Self::U8
                | Self::Bool8
                | Self::I16
                | Self::U16
                | Self::I32
                | Self::U32
                | Self::I64
                | Self::U64
        )
    }

    fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Named { .. }
                | Self::FixedArray { .. }
                | Self::CountedArray { .. }
                | Self::RecursiveArray { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub ty: FieldType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<Condition>,
}

impl FieldDef {
    pub fn new(name: impl Into<String>, ty: FieldType) -> Self {
        Self { name: name.into(), ty, when: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDef {
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub root: String,
    pub structs: BTreeMap<String, StructDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Version {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u32>,
    },
    Field { field: String, predicate: Predicate },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", content = "value", rename_all = "snake_case")]
pub enum Predicate {
    Eq(i128),
    NotEq(i128),
    OneOf(Vec<i128>),
    Zero,
    NonZero,
    AtLeast(i128),
    AtMost(i128),
    BitsAny(u128),
    BitsAll(u128),
}

impl Predicate {
    pub(crate) fn matches(&self, value: i128) -> bool {
        match self {
            Self::Eq(expected) => value == *expected,
            Self::NotEq(expected) => value != *expected,
            Self::OneOf(values) => values.contains(&value),
            Self::Zero => value == 0,
            Self::NonZero => value != 0,
            Self::AtLeast(minimum) => value >= *minimum,
            Self::AtMost(maximum) => value <= *maximum,
            Self::BitsAny(mask) => (value as u128 & mask) != 0,
            Self::BitsAll(mask) => (value as u128 & mask) == *mask,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionedSchema {
    pub base_version: u32,
    pub schema: Schema,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patches: Vec<VersionPatch>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionPatch {
    pub min_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_version: Option<u32>,
    pub operations: Vec<PatchOperation>,
}

impl VersionPatch {
    fn applies(&self, version: u32) -> bool {
        version >= self.min_version && self.max_version.map_or(true, |maximum| version <= maximum)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum PatchOperation {
    InsertAfter {
        structure: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
        field: FieldDef,
    },
    Remove { structure: String, field: String },
    Replace { structure: String, field: String, replacement: FieldDef },
}

impl VersionedSchema {
    /// Applies matching patches in declaration order. A patch can therefore
    /// anchor an insertion to a field introduced by an earlier patch.
    pub fn resolve(&self, version: u32) -> Result<Schema, String> {
        if version < self.base_version {
            return Err(format!("task schema starts at v{}, not v{version}", self.base_version));
        }
        let mut schema = self.schema.clone();
        for patch in self.patches.iter().filter(|patch| patch.applies(version)) {
            for operation in &patch.operations {
                apply_patch(&mut schema, operation)?;
            }
        }
        schema.validate()?;
        Ok(schema)
    }
}

fn apply_patch(schema: &mut Schema, operation: &PatchOperation) -> Result<(), String> {
    match operation {
        PatchOperation::InsertAfter { structure, after, field } => {
            let target = schema.structs.get_mut(structure)
                .ok_or_else(|| format!("task schema patch names unknown structure {structure:?}"))?;
            if target.fields.iter().any(|candidate| candidate.name == field.name) {
                return Err(format!("task schema patch would duplicate {structure}.{}", field.name));
            }
            let at = match after {
                Some(anchor) => target.fields.iter().position(|candidate| candidate.name == *anchor)
                    .map(|index| index + 1)
                    .ok_or_else(|| format!("task schema patch cannot find {structure}.{anchor}"))?,
                None => 0,
            };
            target.fields.insert(at, field.clone());
        }
        PatchOperation::Remove { structure, field } => {
            let target = schema.structs.get_mut(structure)
                .ok_or_else(|| format!("task schema patch names unknown structure {structure:?}"))?;
            let at = target.fields.iter().position(|candidate| candidate.name == *field)
                .ok_or_else(|| format!("task schema patch cannot find {structure}.{field}"))?;
            target.fields.remove(at);
        }
        PatchOperation::Replace { structure, field, replacement } => {
            let target = schema.structs.get_mut(structure)
                .ok_or_else(|| format!("task schema patch names unknown structure {structure:?}"))?;
            let at = target.fields.iter().position(|candidate| candidate.name == *field)
                .ok_or_else(|| format!("task schema patch cannot find {structure}.{field}"))?;
            target.fields[at] = replacement.clone();
        }
    }
    Ok(())
}

impl Schema {
    pub fn with_operations(&self, operations: &[PatchOperation]) -> Result<Self, String> {
        let mut schema = self.clone();
        for operation in operations {
            apply_patch(&mut schema, operation)?;
        }
        schema.validate()?;
        Ok(schema)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.structs.contains_key(&self.root) {
            return Err(format!("task schema root {:?} does not exist", self.root));
        }
        for (name, structure) in &self.structs {
            let mut prior = HashMap::<&str, &FieldType>::new();
            let mut names = HashSet::new();
            for field in &structure.fields {
                let path = format!("{name}.{}", field.name);
                if field.name.trim().is_empty() {
                    return Err(format!("{name}: a field has no name"));
                }
                if !names.insert(field.name.as_str()) {
                    return Err(format!("{path}: duplicate field name"));
                }
                validate_type(self, &field.ty, &prior, &path)?;
                for condition in &field.when {
                    if let Condition::Field { field, .. } = condition {
                        let Some(numeric) = numeric_reference(self, &prior, field) else {
                            return Err(format!("{path}: condition refers to {field:?} before it is read"));
                        };
                        if !numeric {
                            return Err(format!("{path}: condition field {field:?} is not an integer"));
                        }
                    }
                }
                prior.insert(&field.name, &field.ty);
            }
        }
        Ok(())
    }
}

fn validate_type<'a>(
    schema: &Schema,
    ty: &'a FieldType,
    prior: &HashMap<&str, &'a FieldType>,
    path: &str,
) -> Result<(), String> {
    match ty {
        FieldType::FixedUtf16 { units } if *units == 0 => {
            Err(format!("{path}: fixed text must contain at least one UTF-16 unit"))
        }
        FieldType::Bytes { len } | FieldType::Raw { len } if *len == 0 => {
            Err(format!("{path}: byte width must be at least one"))
        }
        FieldType::Named { name } => {
            if schema.structs.contains_key(name) { Ok(()) } else { Err(format!("{path}: unknown structure {name:?}")) }
        }
        FieldType::FixedArray { len, item } => {
            if *len == 0 {
                return Err(format!("{path}: array length must be at least one"));
            }
            validate_type(schema, item, prior, path)
        }
        FieldType::CountedArray { count_field, item } => {
            validate_count_field(schema, prior, count_field, path)?;
            validate_type(schema, item, prior, path)
        }
        FieldType::CountedUtf16 { count_field, .. } | FieldType::CountedBytes { count_field } => {
            validate_count_field(schema, prior, count_field, path)
        }
        FieldType::RecursiveArray { count_field, target } => {
            validate_count_field(schema, prior, count_field, path)?;
            if schema.structs.contains_key(target) { Ok(()) } else { Err(format!("{path}: unknown recursive structure {target:?}")) }
        }
        _ => Ok(()),
    }
}

fn validate_count_field(
    schema: &Schema,
    prior: &HashMap<&str, &FieldType>,
    count_field: &str,
    path: &str,
) -> Result<(), String> {
    let Some(numeric) = numeric_reference(schema, prior, count_field) else {
        return Err(format!("{path}: count field {count_field:?} must appear earlier in the same structure"));
    };
    if !numeric {
        return Err(format!("{path}: count field {count_field:?} is not an integer"));
    }
    Ok(())
}

fn numeric_reference(schema: &Schema, prior: &HashMap<&str, &FieldType>, reference: &str) -> Option<bool> {
    let mut parts = reference.split('.');
    let mut ty = *prior.get(parts.next()?)?;
    for part in parts {
        let FieldType::Named { name } = ty else { return None };
        ty = &schema.structs.get(name)?.fields.iter().find(|field| field.name == part)?.ty;
    }
    Some(ty.is_numeric_scalar())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
    Struct(Vec<Node>),
    Array(Vec<Node>),
}

impl Value {
    fn integer(&self) -> Option<i128> {
        match self {
            Self::I64(value) => Some(*value as i128),
            Self::U64(value) => Some(*value as i128),
            Self::Bool(value) => Some(i128::from(*value)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub name: String,
    pub offset: usize,
    pub byte_len: usize,
    pub ty: FieldType,
    pub value: Value,
    #[serde(skip)]
    original: Option<Vec<u8>>,
    #[serde(skip)]
    dirty: bool,
}

impl Node {
    pub fn children(&self) -> &[Node] {
        match &self.value {
            Value::Struct(children) | Value::Array(children) => children,
            _ => &[],
        }
    }

    pub fn children_mut(&mut self) -> &mut [Node] {
        match &mut self.value {
            Value::Struct(children) | Value::Array(children) => children,
            _ => &mut [],
        }
    }

    pub fn array_mut(&mut self) -> Option<&mut Vec<Node>> {
        match &mut self.value {
            Value::Array(children) => Some(children),
            _ => None,
        }
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children().iter().find(|node| node.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Node> {
        self.children_mut().iter_mut().find(|node| node.name == name)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty || self.children().iter().any(Node::is_dirty)
    }

    /// Changes one leaf after checking that its representation still fits the
    /// declared binary type. Container changes are handled through children.
    pub fn set_value(&mut self, value: Value) -> Result<(), String> {
        if self.ty.is_container() {
            return Err(format!("{} is a container", self.name));
        }
        encode_leaf(&self.ty, &value)?;
        self.value = value;
        self.dirty = true;
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        match &self.value {
            Value::Struct(children) | Value::Array(children) => {
                let mut output = Vec::with_capacity(self.byte_len);
                for child in children {
                    output.extend(child.encode()?);
                }
                Ok(output)
            }
            _ if !self.dirty => self.original.clone().ok_or_else(|| format!("{} has no original bytes", self.name)),
            value => encode_leaf(&self.ty, value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_depth: usize,
    pub max_array_items: usize,
    pub max_nodes: usize,
    pub max_text_units: usize,
    pub max_blob_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 128,
            max_array_items: 1_000_000,
            max_nodes: 2_000_000,
            max_text_units: 4_000_000,
            max_blob_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Field names (or array names) whose values are task IDs: award and
/// prerequisite links between tasks. Zero means "none".
pub fn is_task_reference(semantic: &str) -> bool {
    matches!(
        semantic,
        "task_id" | "new_task_id" | "terminate_task_ids" | "premise_tasks" | "mutex_tasks" | "premise_global_task" | "premise_cotask"
    )
}

/// Field names whose values are elements.data essence IDs (items, monsters,
/// NPCs, interaction objects).
pub fn is_element_reference(semantic: &str) -> bool {
    matches!(
        semantic,
        "item_id" | "drop_item_id" | "travel_item_id" | "replacement_item_id" | "monster_id" | "object_id"
            // Header fields named from TaskTempl.h: NPCs, monsters and items.
            | "deliver_npc" | "award_npc" | "action_npc" | "npc_to_protect" | "npc_moving"
            | "kill_fail_monsters" | "have_fail_items" | "not_have_fail_items"
    )
}

/// NPC functions (`SERVICE_TYPE` in ExpTypes.h, minus the 0x80000000 flag) whose talk option
/// `parameter` is a task ID: NPC_TALK, NPC_GIVE_TASK, NPC_COMPLETE_TASK,
/// NPC_GIVE_TASK_MATTER and TALK_GIVEUP_TASK. On the fixtures every non-zero parameter of these
/// options but three names an existing task.
pub const TASK_OPTION_FUNCTIONS: [u64; 5] = [0, 6, 7, 8, 21];

/// Whether a talk option ID runs a function whose parameter is a task ID.
pub fn is_task_option_function(option_id: i128) -> bool {
    option_id >= 0x8000_0000 && TASK_OPTION_FUNCTIONS.contains(&((option_id - 0x8000_0000) as u64))
}

/// The four hierarchy link fields at the end of every task's fixed block, in
/// order: parent, previous sibling, next sibling, first child.
pub const LINK_FIELDS: [&str; 4] = ["hierarchy_parent", "hierarchy_previous_sibling", "hierarchy_next_sibling", "hierarchy_first_child"];

pub fn decode_exact(schema: &Schema, bytes: &[u8], version: u32) -> Result<Node, String> {
    let (node, used) = decode_prefix_with_limits(schema, bytes, version, Limits::default())?;
    if used != bytes.len() {
        return Err(format!("task record has {} trailing bytes after byte {used}", bytes.len() - used));
    }
    Ok(node)
}

pub fn decode_prefix(schema: &Schema, bytes: &[u8], version: u32) -> Result<(Node, usize), String> {
    decode_prefix_with_limits(schema, bytes, version, Limits::default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeFailure {
    /// The byte position reached before the baseline schema could not continue.
    pub offset: usize,
    pub message: String,
}

/// Decodes as far as possible while retaining the byte position of a failure.
/// The unsupported-version analyzer uses this to distinguish a complete root,
/// trailing bytes, and a structural failure without changing the normal parser.
pub fn decode_prefix_diagnostic(schema: &Schema, bytes: &[u8], version: u32) -> Result<(Node, usize), DecodeFailure> {
    schema.validate().map_err(|message| DecodeFailure { offset: 0, message })?;
    let root = schema.root.clone();
    let mut decoder = Decoder { schema, bytes, version, limits: Limits::default(), position: 0, nodes: 0 };
    match decoder.named(&root, &root, &root, 0) {
        Ok(node) => Ok((node, decoder.position)),
        Err(message) => Err(DecodeFailure { offset: decoder.position, message }),
    }
}

pub fn decode_prefix_with_limits(
    schema: &Schema,
    bytes: &[u8],
    version: u32,
    limits: Limits,
) -> Result<(Node, usize), String> {
    schema.validate()?;
    let root = schema.root.clone();
    let mut decoder = Decoder { schema, bytes, version, limits, position: 0, nodes: 0 };
    let node = decoder.named(&root, &root, &root, 0)?;
    Ok((node, decoder.position))
}

/// Reads one integer field from the root structure without constructing the
/// decoded field tree. This is used for inexpensive list metadata such as a
/// root task's `subtask_count`.
pub fn probe_root_integer(schema: &Schema, bytes: &[u8], version: u32, field_name: &str) -> Result<i128, String> {
    schema.validate()?;
    probe_root_integer_validated(schema, bytes, version, field_name)
}

pub(crate) fn probe_root_integer_validated(schema: &Schema, bytes: &[u8], version: u32, field_name: &str) -> Result<i128, String> {
    let definition = schema.structs.get(&schema.root)
        .ok_or_else(|| format!("unknown root structure {:?}", schema.root))?;
    let mut decoder = ProbeDecoder { schema, bytes, version, limits: Limits::default(), position: 0, tasks: None, task_path: Vec::new(), task_stack: Vec::new(), task_bases: Vec::new(), needed: probe_needed_names(schema) };
    let mut scope = HashMap::<String, i128>::new();
    for field in &definition.fields {
        let path = format!("{}.{}", schema.root, field.name);
        if !conditions_match(&field.when, version, &scope, &path)? {
            continue;
        }
        let numeric = decoder.value(&field.ty, &path, &scope, 1)?;
        if field.name == field_name {
            return match numeric {
                ProbeNumeric::Integer(value) => Ok(value),
                _ => Err(format!("{path}: requested field is not an integer")),
            };
        }
        collect_probe_numeric(&field.name, numeric, &mut scope, &decoder.needed);
    }
    Err(format!("root structure {:?} has no field {field_name:?}", schema.root))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbedTask {
    pub id: u32,
    pub name: String,
    pub path: Vec<usize>,
    pub child_count: usize,
    pub references: Vec<ProbedTaskReference>,
    /// The stored hierarchy links (see `LINK_FIELDS`), when the layout names them.
    pub links: Option<[u32; 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbedTaskReference {
    /// Dotted path from the task, such as `fixed.premise_tasks[0]`.
    pub field: String,
    pub target_id: u32,
    /// An elements.data essence ID rather than a task ID.
    pub element: bool,
}

pub(crate) fn probe_task_index_validated(schema: &Schema, bytes: &[u8], version: u32) -> Result<Vec<ProbedTask>, String> {
    let root = schema.root.clone();
    let mut decoder = ProbeDecoder {
        schema,
        bytes,
        version,
        limits: Limits::default(),
        position: 0,
        tasks: Some(Vec::new()),
        task_path: Vec::new(),
        task_stack: Vec::new(),
        task_bases: Vec::new(),
        needed: probe_needed_names(schema),
    };
    decoder.named(&root, &root, 0)?;
    if decoder.position != bytes.len() {
        return Err(format!("task record has {} trailing bytes after byte {}", bytes.len() - decoder.position, decoder.position));
    }
    Ok(decoder.tasks.unwrap_or_default())
}

enum ProbeNumeric {
    None,
    Integer(i128),
    Struct(HashMap<String, i128>),
}

/// Every name a condition or count can look up, with each dotted suffix
/// (`fixed.has_signature` also needs `has_signature` inside `fixed`). The probe
/// keeps only these, instead of every numeric field of the header.
fn probe_needed_names(schema: &Schema) -> HashSet<String> {
    fn add(name: &str, needed: &mut HashSet<String>) {
        let mut rest = name;
        loop {
            needed.insert(rest.to_string());
            match rest.split_once('.') {
                Some((_, tail)) => rest = tail,
                None => break,
            }
        }
    }
    fn walk(ty: &FieldType, needed: &mut HashSet<String>) {
        match ty {
            FieldType::CountedArray { count_field, item } => {
                add(count_field, needed);
                walk(item, needed);
            }
            FieldType::FixedArray { item, .. } => walk(item, needed),
            FieldType::RecursiveArray { count_field, .. } | FieldType::CountedUtf16 { count_field, .. } | FieldType::CountedBytes { count_field } => add(count_field, needed),
            _ => {}
        }
    }
    let mut needed = HashSet::new();
    add("subtask_count", &mut needed);
    // Talk options: `parameter` is a task reference when `id` names a task function.
    add("id", &mut needed);
    for definition in schema.structs.values() {
        for field in &definition.fields {
            walk(&field.ty, &mut needed);
            for condition in &field.when {
                if let Condition::Field { field, .. } = condition {
                    add(field, &mut needed);
                }
            }
        }
    }
    needed
}

fn collect_probe_numeric(name: &str, numeric: ProbeNumeric, scope: &mut HashMap<String, i128>, needed: &HashSet<String>) {
    match numeric {
        ProbeNumeric::None => {}
        ProbeNumeric::Integer(value) => {
            if needed.contains(name) {
                scope.insert(name.to_string(), value);
            }
        }
        ProbeNumeric::Struct(values) => {
            for (child, value) in values {
                let key = format!("{name}.{child}");
                if needed.contains(&key) {
                    scope.insert(key, value);
                }
            }
        }
    }
}

struct ProbeDecoder<'a> {
    schema: &'a Schema,
    bytes: &'a [u8],
    version: u32,
    limits: Limits,
    position: usize,
    tasks: Option<Vec<ProbedTask>>,
    task_path: Vec<usize>,
    task_stack: Vec<usize>,
    task_bases: Vec<String>,
    /// Names conditions and counts read (see `probe_needed_names`).
    needed: HashSet<String>,
}

impl ProbeDecoder<'_> {
    fn named(&mut self, structure: &str, path: &str, depth: usize) -> Result<HashMap<String, i128>, String> {
        self.check_depth(depth, path)?;
        let definition = self.schema.structs.get(structure)
            .ok_or_else(|| format!("{path}: unknown structure {structure:?}"))?;
        let task_index = if self.tasks.is_some() && structure == self.schema.root {
            let heading = self.bytes.get(self.position..self.position.saturating_add(64))
                .ok_or_else(|| format!("{path}: task heading is truncated at byte {}", self.position))?;
            let units = heading[4..].chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>();
            let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
            let task = ProbedTask {
                id: u32::from_le_bytes(heading[0..4].try_into().unwrap()),
                name: String::from_utf16_lossy(&units[..end]),
                path: self.task_path.clone(),
                child_count: 0,
                references: Vec::new(),
                links: None,
            };
            let tasks = self.tasks.as_mut().unwrap();
            tasks.push(task);
            let index = tasks.len() - 1;
            self.task_stack.push(index);
            self.task_bases.push(path.to_string());
            Some(index)
        } else {
            None
        };
        let mut scope = HashMap::<String, i128>::new();
        for field in &definition.fields {
            if !conditions_match(&field.when, self.version, &scope, path)? {
                continue;
            }
            let child_path = format!("{path}.{}", field.name);
            let numeric = self.value(&field.ty, &child_path, &scope, depth + 1)?;
            collect_probe_numeric(&field.name, numeric, &mut scope, &self.needed);
        }
        if let Some(index) = task_index {
            let count = scope.get("subtask_count").copied().unwrap_or(0);
            let child_count = usize::try_from(count).map_err(|_| format!("{path}: invalid subtask count {count}"))?;
            self.tasks.as_mut().unwrap()[index].child_count = child_count;
            self.task_stack.pop();
            self.task_bases.pop();
        }
        Ok(scope)
    }

    fn value(
        &mut self,
        ty: &FieldType,
        path: &str,
        scope: &HashMap<String, i128>,
        depth: usize,
    ) -> Result<ProbeNumeric, String> {
        self.check_depth(depth, path)?;
        match ty {
            FieldType::Named { name } => Ok(ProbeNumeric::Struct(self.named(name, path, depth)?)),
            FieldType::FixedArray { len, item } => {
                self.array(*len, item, path, scope, depth)?;
                Ok(ProbeNumeric::None)
            }
            FieldType::CountedArray { count_field, item } => {
                let count = array_count(scope, count_field, path, self.limits.max_array_items)?;
                self.array(count, item, path, scope, depth)?;
                Ok(ProbeNumeric::None)
            }
            FieldType::RecursiveArray { count_field, target } => {
                let count = array_count(scope, count_field, path, self.limits.max_array_items)?;
                for index in 0..count {
                    let tracks_task = self.tasks.is_some() && target == &self.schema.root;
                    if tracks_task {
                        self.task_path.push(index);
                    }
                    let result = self.named(target, &format!("{path}[{index}]"), depth + 1);
                    if tracks_task {
                        self.task_path.pop();
                    }
                    result?;
                }
                Ok(ProbeNumeric::None)
            }
            FieldType::CountedUtf16 { count_field, unit } => {
                let count = array_count(scope, count_field, path, self.limits.max_text_units.saturating_mul(2))?;
                let units = match unit {
                    TextLengthUnit::Utf16Units => count,
                    TextLengthUnit::Bytes if count % 2 == 0 => count / 2,
                    TextLengthUnit::Bytes => return Err(format!("{path}: odd UTF-16 byte length {count}")),
                };
                self.check_text(units, path)?;
                self.take(units.checked_mul(2).ok_or_else(|| format!("{path}: UTF-16 byte length overflow"))?, path)?;
                Ok(ProbeNumeric::None)
            }
            FieldType::CountedBytes { count_field } => {
                let count = array_count(scope, count_field, path, self.limits.max_blob_bytes)?;
                self.take(count, path)?;
                Ok(ProbeNumeric::None)
            }
            _ => self.leaf(ty, path, scope),
        }
    }

    fn array(
        &mut self,
        count: usize,
        item: &FieldType,
        path: &str,
        scope: &HashMap<String, i128>,
        depth: usize,
    ) -> Result<(), String> {
        if count > self.limits.max_array_items {
            return Err(format!("{path}: array has {count} items; limit is {}", self.limits.max_array_items));
        }
        for index in 0..count {
            self.value(item, &format!("{path}[{index}]"), scope, depth + 1)?;
        }
        Ok(())
    }

    fn leaf(&mut self, ty: &FieldType, path: &str, scope: &HashMap<String, i128>) -> Result<ProbeNumeric, String> {
        let numeric = match ty {
            FieldType::I8 => Some(i8::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::U8 => Some(u8::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::Bool8 => Some((u8::from_le_bytes(self.take_array(path)?) != 0) as i128),
            FieldType::I16 => Some(i16::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::U16 => Some(u16::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::I32 => Some(i32::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::U32 => Some(u32::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::I64 => Some(i64::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::U64 => Some(u64::from_le_bytes(self.take_array(path)?) as i128),
            FieldType::F32 => { self.take(4, path)?; None }
            FieldType::F64 => { self.take(8, path)?; None }
            FieldType::FixedUtf16 { units } => {
                self.check_text(*units, path)?;
                self.take(units.checked_mul(2).ok_or_else(|| format!("{path}: UTF-16 byte length overflow"))?, path)?;
                None
            }
            FieldType::PrefixedUtf16 { prefix, unit, terminated } => {
                let count = self.read_count(*prefix, path)?;
                let units = match unit {
                    TextLengthUnit::Utf16Units => count,
                    TextLengthUnit::Bytes if count % 2 == 0 => count / 2,
                    TextLengthUnit::Bytes => return Err(format!("{path}: odd UTF-16 byte length {count}")),
                };
                self.check_text(units, path)?;
                let value = self.take(units.checked_mul(2).ok_or_else(|| format!("{path}: UTF-16 byte length overflow"))?, path)?;
                if *terminated && value.get(value.len().saturating_sub(2)..) != Some(&[0, 0]) {
                    return Err(format!("{path}: length-prefixed text has no terminator"));
                }
                None
            }
            FieldType::Bytes { len } | FieldType::Raw { len } => {
                if *len > self.limits.max_blob_bytes {
                    return Err(format!("{path}: byte block has {len} bytes; limit is {}", self.limits.max_blob_bytes));
                }
                self.take(*len, path)?;
                None
            }
            _ => return Err(format!("{path}: expected a leaf type")),
        };
        if let Some(value) = numeric {
            let semantic = path.rsplit('.').next().unwrap_or(path).split('[').next().unwrap_or("");
            let element = is_element_reference(semantic);
            let link = LINK_FIELDS.iter().position(|name| *name == semantic);
            let option_task = semantic == "parameter" && path.contains(".options[") && scope.get("id").is_some_and(|id| is_task_option_function(*id));
            if is_task_reference(semantic) || option_task || element || link.is_some() {
                if let (Some(index), Ok(target_id)) = (self.task_stack.last().copied(), u32::try_from(value)) {
                    let base = self.task_bases.last().map(String::as_str).unwrap_or("");
                    let field = path.strip_prefix(base).unwrap_or(path).trim_start_matches('.').to_string();
                    let task = &mut self.tasks.as_mut().unwrap()[index];
                    match link {
                        Some(slot) if field == format!("fixed.{semantic}") => task.links.get_or_insert([0; 4])[slot] = target_id,
                        Some(_) => {}
                        None => task.references.push(ProbedTaskReference { field, target_id, element }),
                    }
                }
            }
            Ok(ProbeNumeric::Integer(value))
        } else {
            Ok(ProbeNumeric::None)
        }
    }

    fn check_depth(&self, depth: usize, path: &str) -> Result<(), String> {
        if depth > self.limits.max_depth {
            Err(format!("{path}: nesting is deeper than {} levels", self.limits.max_depth))
        } else {
            Ok(())
        }
    }

    fn check_text(&self, units: usize, path: &str) -> Result<(), String> {
        if units > self.limits.max_text_units {
            Err(format!("{path}: text has {units} UTF-16 units; limit is {}", self.limits.max_text_units))
        } else {
            Ok(())
        }
    }

    fn take(&mut self, len: usize, path: &str) -> Result<&[u8], String> {
        let end = self.position.checked_add(len).ok_or_else(|| format!("{path}: byte offset overflow"))?;
        let bytes = self.bytes.get(self.position..end).ok_or_else(|| {
            format!("{path}: needs {len} bytes at offset {}, but the record ends at {}", self.position, self.bytes.len())
        })?;
        self.position = end;
        Ok(bytes)
    }

    fn take_array<const N: usize>(&mut self, path: &str) -> Result<[u8; N], String> {
        Ok(self.take(N, path)?.try_into().unwrap())
    }

    fn read_count(&mut self, width: CountWidth, path: &str) -> Result<usize, String> {
        match width {
            CountWidth::U8 => Ok(u8::from_le_bytes(self.take_array(path)?) as usize),
            CountWidth::U16 => Ok(u16::from_le_bytes(self.take_array(path)?) as usize),
            CountWidth::U32 => usize::try_from(u32::from_le_bytes(self.take_array(path)?))
                .map_err(|_| format!("{path}: text length does not fit this computer")),
        }
    }
}

struct Decoder<'a> {
    schema: &'a Schema,
    bytes: &'a [u8],
    version: u32,
    limits: Limits,
    position: usize,
    nodes: usize,
}

impl Decoder<'_> {
    fn named(&mut self, structure: &str, node_name: &str, path: &str, depth: usize) -> Result<Node, String> {
        self.check_depth(depth, path)?;
        let definition = self.schema.structs.get(structure)
            .ok_or_else(|| format!("{path}: unknown structure {structure:?}"))?;
        let start = self.position;
        let mut scope = HashMap::<String, i128>::new();
        let mut children = Vec::with_capacity(definition.fields.len());
        for field in &definition.fields {
            if !conditions_match(&field.when, self.version, &scope, path)? {
                continue;
            }
            let child_path = format!("{path}.{}", field.name);
            let child = self.value(&field.ty, &field.name, &child_path, &scope, depth + 1)?;
            collect_numeric(&child, &field.name, &mut scope);
            children.push(child);
        }
        self.node(
            node_name,
            start,
            FieldType::Named { name: structure.to_string() },
            Value::Struct(children),
            None,
        )
    }

    fn value(
        &mut self,
        ty: &FieldType,
        name: &str,
        path: &str,
        scope: &HashMap<String, i128>,
        depth: usize,
    ) -> Result<Node, String> {
        self.check_depth(depth, path)?;
        match ty {
            FieldType::Named { name: structure } => self.named(structure, name, path, depth),
            FieldType::FixedArray { len, item } => self.array(name, ty, *len, item, path, scope, depth),
            FieldType::CountedArray { count_field, item } => {
                let count = array_count(scope, count_field, path, self.limits.max_array_items)?;
                self.array(name, ty, count, item, path, scope, depth)
            }
            FieldType::RecursiveArray { count_field, target } => {
                let count = array_count(scope, count_field, path, self.limits.max_array_items)?;
                let start = self.position;
                let mut children = Vec::with_capacity(count);
                for index in 0..count {
                    let child_name = format!("[{index}]");
                    children.push(self.named(target, &child_name, &format!("{path}{child_name}"), depth + 1)?);
                }
                self.node(name, start, ty.clone(), Value::Array(children), None)
            }
            FieldType::CountedUtf16 { count_field, unit } => {
                let count = array_count(scope, count_field, path, self.limits.max_text_units.saturating_mul(2))?;
                let units = match unit {
                    TextLengthUnit::Utf16Units => count,
                    TextLengthUnit::Bytes if count % 2 == 0 => count / 2,
                    TextLengthUnit::Bytes => return Err(format!("{path}: odd UTF-16 byte length {count}")),
                };
                self.check_text(units, path)?;
                let start = self.position;
                let len = units.checked_mul(2).ok_or_else(|| format!("{path}: UTF-16 byte length overflow"))?;
                let original = self.take(len, path)?.to_vec();
                let value = Value::Text(String::from_utf16_lossy(&utf16_units(&original)));
                self.node(name, start, ty.clone(), value, Some(original))
            }
            FieldType::CountedBytes { count_field } => {
                let count = array_count(scope, count_field, path, self.limits.max_blob_bytes)?;
                let start = self.position;
                let original = self.take(count, path)?.to_vec();
                self.node(name, start, ty.clone(), Value::Bytes(original.clone()), Some(original))
            }
            _ => self.leaf(ty, name, path),
        }
    }

    fn array(
        &mut self,
        name: &str,
        ty: &FieldType,
        count: usize,
        item: &FieldType,
        path: &str,
        scope: &HashMap<String, i128>,
        depth: usize,
    ) -> Result<Node, String> {
        if count > self.limits.max_array_items {
            return Err(format!("{path}: array has {count} items; limit is {}", self.limits.max_array_items));
        }
        let start = self.position;
        let mut children = Vec::with_capacity(count);
        for index in 0..count {
            let child_name = format!("[{index}]");
            children.push(self.value(item, &child_name, &format!("{path}{child_name}"), scope, depth + 1)?);
        }
        self.node(name, start, ty.clone(), Value::Array(children), None)
    }

    fn leaf(&mut self, ty: &FieldType, name: &str, path: &str) -> Result<Node, String> {
        let start = self.position;
        let value = match ty {
            FieldType::I8 => Value::I64(i8::from_le_bytes(self.take_array(path)?) as i64),
            FieldType::U8 => Value::U64(u8::from_le_bytes(self.take_array(path)?) as u64),
            FieldType::Bool8 => Value::Bool(u8::from_le_bytes(self.take_array(path)?) != 0),
            FieldType::I16 => Value::I64(i16::from_le_bytes(self.take_array(path)?) as i64),
            FieldType::U16 => Value::U64(u16::from_le_bytes(self.take_array(path)?) as u64),
            FieldType::I32 => Value::I64(i32::from_le_bytes(self.take_array(path)?) as i64),
            FieldType::U32 => Value::U64(u32::from_le_bytes(self.take_array(path)?) as u64),
            FieldType::I64 => Value::I64(i64::from_le_bytes(self.take_array(path)?)),
            FieldType::U64 => Value::U64(u64::from_le_bytes(self.take_array(path)?)),
            FieldType::F32 => Value::F32(f32::from_le_bytes(self.take_array(path)?)),
            FieldType::F64 => Value::F64(f64::from_le_bytes(self.take_array(path)?)),
            FieldType::FixedUtf16 { units } => {
                self.check_text(*units, path)?;
                let len = units.checked_mul(2).ok_or_else(|| format!("{path}: UTF-16 byte length overflow"))?;
                let values = utf16_units(self.take(len, path)?);
                let end = values.iter().position(|unit| *unit == 0).unwrap_or(values.len());
                Value::Text(String::from_utf16_lossy(&values[..end]))
            }
            FieldType::PrefixedUtf16 { prefix, unit, terminated } => {
                let count = self.read_count(*prefix, path)?;
                let units = match unit {
                    TextLengthUnit::Utf16Units => count,
                    TextLengthUnit::Bytes if count % 2 == 0 => count / 2,
                    TextLengthUnit::Bytes => return Err(format!("{path}: odd UTF-16 byte length {count}")),
                };
                self.check_text(units, path)?;
                let len = units.checked_mul(2).ok_or_else(|| format!("{path}: UTF-16 byte length overflow"))?;
                let mut values = utf16_units(self.take(len, path)?);
                if *terminated {
                    if values.last() != Some(&0) {
                        return Err(format!("{path}: length-prefixed text has no terminator"));
                    }
                    values.pop();
                }
                Value::Text(String::from_utf16_lossy(&values))
            }
            FieldType::Bytes { len } | FieldType::Raw { len } => {
                if *len > self.limits.max_blob_bytes {
                    return Err(format!("{path}: byte block has {len} bytes; limit is {}", self.limits.max_blob_bytes));
                }
                Value::Bytes(self.take(*len, path)?.to_vec())
            }
            _ => return Err(format!("{path}: expected a leaf type")),
        };
        let original = self.bytes[start..self.position].to_vec();
        self.node(name, start, ty.clone(), value, Some(original))
    }

    fn node(
        &mut self,
        name: &str,
        start: usize,
        ty: FieldType,
        value: Value,
        original: Option<Vec<u8>>,
    ) -> Result<Node, String> {
        self.nodes = self.nodes.checked_add(1).ok_or("task schema node count overflow")?;
        if self.nodes > self.limits.max_nodes {
            return Err(format!("task record has more than {} decoded nodes", self.limits.max_nodes));
        }
        Ok(Node {
            name: name.to_string(),
            offset: start,
            byte_len: self.position - start,
            ty,
            value,
            original,
            dirty: false,
        })
    }

    fn check_depth(&self, depth: usize, path: &str) -> Result<(), String> {
        if depth > self.limits.max_depth {
            Err(format!("{path}: nesting is deeper than {} levels", self.limits.max_depth))
        } else {
            Ok(())
        }
    }

    fn check_text(&self, units: usize, path: &str) -> Result<(), String> {
        if units > self.limits.max_text_units {
            Err(format!("{path}: text has {units} UTF-16 units; limit is {}", self.limits.max_text_units))
        } else {
            Ok(())
        }
    }

    fn take(&mut self, len: usize, path: &str) -> Result<&[u8], String> {
        let end = self.position.checked_add(len).ok_or_else(|| format!("{path}: byte offset overflow"))?;
        let bytes = self.bytes.get(self.position..end).ok_or_else(|| {
            format!("{path}: needs {len} bytes at offset {}, but the record ends at {}", self.position, self.bytes.len())
        })?;
        self.position = end;
        Ok(bytes)
    }

    fn take_array<const N: usize>(&mut self, path: &str) -> Result<[u8; N], String> {
        Ok(self.take(N, path)?.try_into().unwrap())
    }

    fn read_count(&mut self, width: CountWidth, path: &str) -> Result<usize, String> {
        match width {
            CountWidth::U8 => Ok(u8::from_le_bytes(self.take_array(path)?) as usize),
            CountWidth::U16 => Ok(u16::from_le_bytes(self.take_array(path)?) as usize),
            CountWidth::U32 => usize::try_from(u32::from_le_bytes(self.take_array(path)?))
                .map_err(|_| format!("{path}: text length does not fit this computer")),
        }
    }
}

fn collect_numeric(node: &Node, path: &str, scope: &mut HashMap<String, i128>) {
    if let Some(value) = node.value.integer() {
        scope.insert(path.to_string(), value);
        return;
    }
    if let Value::Struct(children) = &node.value {
        for child in children {
            collect_numeric(child, &format!("{path}.{}", child.name), scope);
        }
    }
}

fn conditions_match(
    conditions: &[Condition],
    version: u32,
    scope: &HashMap<String, i128>,
    path: &str,
) -> Result<bool, String> {
    for condition in conditions {
        let matches = match condition {
            Condition::Version { min, max } => {
                min.map_or(true, |minimum| version >= minimum)
                    && max.map_or(true, |maximum| version <= maximum)
            }
            Condition::Field { field, predicate } => {
                let value = scope.get(field)
                    .ok_or_else(|| format!("{path}: condition field {field:?} has not been read"))?;
                predicate.matches(*value)
            }
        };
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

fn array_count(
    scope: &HashMap<String, i128>,
    field: &str,
    path: &str,
    limit: usize,
) -> Result<usize, String> {
    let value = *scope.get(field)
        .ok_or_else(|| format!("{path}: count field {field:?} has not been read"))?;
    let count = usize::try_from(value)
        .map_err(|_| format!("{path}: count field {field:?} is negative or too large ({value})"))?;
    if count > limit {
        return Err(format!("{path}: array has {count} items; limit is {limit}"));
    }
    Ok(count)
}

fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    bytes.chunks_exact(2)
        .map(|pair| u16::from_le_bytes(pair.try_into().unwrap()))
        .collect()
}

fn encode_leaf(ty: &FieldType, value: &Value) -> Result<Vec<u8>, String> {
    let mismatch = || format!("value does not match binary type {ty:?}");
    Ok(match (ty, value) {
        (FieldType::I8, Value::I64(value)) => i8::try_from(*value).map_err(|_| "int8 value is out of range")?.to_le_bytes().to_vec(),
        (FieldType::U8, Value::U64(value)) => u8::try_from(*value).map_err(|_| "uint8 value is out of range")?.to_le_bytes().to_vec(),
        (FieldType::Bool8, Value::Bool(value)) => vec![u8::from(*value)],
        (FieldType::I16, Value::I64(value)) => i16::try_from(*value).map_err(|_| "int16 value is out of range")?.to_le_bytes().to_vec(),
        (FieldType::U16, Value::U64(value)) => u16::try_from(*value).map_err(|_| "uint16 value is out of range")?.to_le_bytes().to_vec(),
        (FieldType::I32, Value::I64(value)) => i32::try_from(*value).map_err(|_| "int32 value is out of range")?.to_le_bytes().to_vec(),
        (FieldType::U32, Value::U64(value)) => u32::try_from(*value).map_err(|_| "uint32 value is out of range")?.to_le_bytes().to_vec(),
        (FieldType::I64, Value::I64(value)) => value.to_le_bytes().to_vec(),
        (FieldType::U64, Value::U64(value)) => value.to_le_bytes().to_vec(),
        (FieldType::F32, Value::F32(value)) => value.to_le_bytes().to_vec(),
        (FieldType::F64, Value::F64(value)) => value.to_le_bytes().to_vec(),
        (FieldType::FixedUtf16 { units }, Value::Text(value)) => {
            let encoded: Vec<u16> = value.encode_utf16().collect();
            if encoded.len() > *units {
                return Err(format!("text uses {} UTF-16 units but the field holds {units}", encoded.len()));
            }
            let mut output = Vec::with_capacity(units * 2);
            output.extend(encoded.iter().flat_map(|unit| unit.to_le_bytes()));
            output.resize(units * 2, 0);
            output
        }
        (
            FieldType::PrefixedUtf16 { prefix, unit, terminated },
            Value::Text(value),
        ) => {
            let mut encoded: Vec<u16> = value.encode_utf16().collect();
            if *terminated {
                encoded.push(0);
            }
            let count = match unit {
                TextLengthUnit::Utf16Units => encoded.len(),
                TextLengthUnit::Bytes => encoded.len().checked_mul(2).ok_or("UTF-16 byte length overflow")?,
            };
            if count > prefix.maximum() {
                return Err(format!("text length {count} does not fit a {}-byte prefix", prefix.bytes()));
            }
            let mut output = match prefix {
                CountWidth::U8 => vec![count as u8],
                CountWidth::U16 => (count as u16).to_le_bytes().to_vec(),
                CountWidth::U32 => (count as u32).to_le_bytes().to_vec(),
            };
            output.extend(encoded.iter().flat_map(|unit| unit.to_le_bytes()));
            output
        }
        (FieldType::CountedUtf16 { .. }, Value::Text(value)) => {
            value.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect()
        }
        (FieldType::Bytes { len } | FieldType::Raw { len }, Value::Bytes(bytes)) => {
            if bytes.len() != *len {
                return Err(format!("byte value has {} bytes but the field holds {len}", bytes.len()));
            }
            bytes.clone()
        }
        (FieldType::CountedBytes { .. }, Value::Bytes(bytes)) => bytes.clone(),
        _ => return Err(mismatch()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, ty: FieldType) -> FieldDef {
        FieldDef::new(name, ty)
    }

    fn schema(fields: Vec<FieldDef>) -> Schema {
        Schema {
            root: "Task".into(),
            structs: BTreeMap::from([("Task".into(), StructDef { fields })]),
        }
    }

    #[test]
    fn decodes_conditions_counted_arrays_text_and_offsets_byte_exact() {
        let layout = schema(vec![
            field("flags", FieldType::U32),
            field("count", FieldType::U8),
            field(
                "values",
                FieldType::CountedArray {
                    count_field: "count".into(),
                    item: Box::new(FieldType::U16),
                },
            ),
            field("name", FieldType::FixedUtf16 { units: 4 }),
            field(
                "description",
                FieldType::PrefixedUtf16 {
                    prefix: CountWidth::U8,
                    unit: TextLengthUnit::Utf16Units,
                    terminated: true,
                },
            ),
            FieldDef {
                name: "flag_value".into(),
                ty: FieldType::Raw { len: 2 },
                when: vec![Condition::Field {
                    field: "flags".into(),
                    predicate: Predicate::BitsAny(0x04),
                }],
            },
            FieldDef {
                name: "new_value".into(),
                ty: FieldType::I32,
                when: vec![Condition::Version { min: Some(200), max: None }],
            },
        ]);

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.push(2);
        bytes.extend_from_slice(&10u16.to_le_bytes());
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend("A".encode_utf16().flat_map(|unit| unit.to_le_bytes()));
        bytes.extend_from_slice(&[0; 6]);
        bytes.push(3);
        bytes.extend("Hi".encode_utf16().flat_map(|unit| unit.to_le_bytes()));
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[0xaa, 0xbb]);

        let mut root = decode_exact(&layout, &bytes, 165).unwrap();
        assert_eq!(root.offset, 0);
        assert_eq!(root.byte_len, bytes.len());
        assert_eq!(root.child("values").unwrap().children().len(), 2);
        assert_eq!(root.child("name").unwrap().offset, 9);
        assert_eq!(root.child("description").unwrap().value, Value::Text("Hi".into()));
        assert!(root.child("new_value").is_none());
        assert_eq!(root.encode().unwrap(), bytes);

        root.child_mut("name").unwrap().set_value(Value::Text("Test".into())).unwrap();
        let edited = root.encode().unwrap();
        assert_eq!(&edited[9..17], &[84, 0, 101, 0, 115, 0, 116, 0]);
        assert!(root.is_dirty());
    }

    #[test]
    fn reusable_structs_and_fixed_arrays_round_trip() {
        let layout = Schema {
            root: "Task".into(),
            structs: BTreeMap::from([
                (
                    "Task".into(),
                    StructDef {
                        fields: vec![field(
                            "points",
                            FieldType::FixedArray {
                                len: 2,
                                item: Box::new(FieldType::Named { name: "Point".into() }),
                            },
                        )],
                    },
                ),
                (
                    "Point".into(),
                    StructDef {
                        fields: vec![field("x", FieldType::F32), field("y", FieldType::F32)],
                    },
                ),
            ]),
        };
        let bytes: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0].into_iter().flat_map(f32::to_le_bytes).collect();
        let root = decode_exact(&layout, &bytes, 165).unwrap();
        assert_eq!(root.child("points").unwrap().children().len(), 2);
        assert_eq!(root.encode().unwrap(), bytes);
    }

    #[test]
    fn recursive_task_trees_are_bounded_and_byte_exact() {
        let layout = schema(vec![
            field("id", FieldType::U32),
            field("child_count", FieldType::U8),
            field(
                "children",
                FieldType::RecursiveArray {
                    count_field: "child_count".into(),
                    target: "Task".into(),
                },
            ),
        ]);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(2);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.push(0);

        let root = decode_exact(&layout, &bytes, 165).unwrap();
        assert_eq!(probe_root_integer(&layout, &bytes, 165, "child_count").unwrap(), 2);
        let children = root.child("children").unwrap().children();
        assert_eq!(children.len(), 2);
        assert_eq!(children[1].child("children").unwrap().children().len(), 1);
        assert_eq!(root.encode().unwrap(), bytes);

        let limits = Limits { max_depth: 2, ..Limits::default() };
        let error = decode_prefix_with_limits(&layout, &bytes, 165, limits).unwrap_err();
        assert!(error.contains("nesting is deeper"), "{error}");
    }

    #[test]
    fn task_index_probe_keeps_references_with_their_owning_task() {
        let layout = schema(vec![
            field("id", FieldType::U32),
            field("name", FieldType::FixedUtf16 { units: 30 }),
            field("task_id", FieldType::U32),
            field("subtask_count", FieldType::U32),
            field(
                "subtasks",
                FieldType::RecursiveArray {
                    count_field: "subtask_count".into(),
                    target: "Task".into(),
                },
            ),
        ]);
        fn task(bytes: &mut Vec<u8>, id: u32, name: &str, target: u32, children: u32) {
            bytes.extend_from_slice(&id.to_le_bytes());
            let mut units = name.encode_utf16().collect::<Vec<_>>();
            units.resize(30, 0);
            bytes.extend(units.into_iter().flat_map(u16::to_le_bytes));
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&children.to_le_bytes());
        }
        let mut bytes = Vec::new();
        task(&mut bytes, 1, "Root", 99, 1);
        task(&mut bytes, 2, "Child", 1, 0);

        let tasks = probe_task_index_validated(&layout, &bytes, 165).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].references, vec![ProbedTaskReference { field: "task_id".into(), target_id: 99, element: false }]);
        assert_eq!(tasks[1].references, vec![ProbedTaskReference { field: "task_id".into(), target_id: 1, element: false }]);
    }

    #[test]
    fn ordered_version_patches_insert_fields_between_existing_fields() {
        let versions = VersionedSchema {
            base_version: 165,
            schema: schema(vec![field("id", FieldType::U32), field("tail", FieldType::U32)]),
            patches: vec![VersionPatch {
                min_version: 203,
                max_version: None,
                operations: vec![PatchOperation::InsertAfter {
                    structure: "Task".into(),
                    after: Some("id".into()),
                    field: field("new_race_combo", FieldType::Raw { len: 4 }),
                }],
            }],
        };
        let old = versions.resolve(202).unwrap();
        let new = versions.resolve(203).unwrap();
        let old_bytes = [1u32.to_le_bytes(), 2u32.to_le_bytes()].concat();
        let new_bytes = [1u32.to_le_bytes(), 99u32.to_le_bytes(), 2u32.to_le_bytes()].concat();
        assert_eq!(decode_exact(&old, &old_bytes, 202).unwrap().encode().unwrap(), old_bytes);
        let decoded = decode_exact(&new, &new_bytes, 203).unwrap();
        assert_eq!(decoded.child("new_race_combo").unwrap().offset, 4);
        assert_eq!(decoded.encode().unwrap(), new_bytes);
    }

    #[test]
    fn rejects_bad_schema_references_and_truncated_records() {
        let bad = schema(vec![field(
            "items",
            FieldType::CountedArray {
                count_field: "missing".into(),
                item: Box::new(FieldType::U32),
            },
        )]);
        assert!(bad.validate().unwrap_err().contains("must appear earlier"));

        let good = schema(vec![field("id", FieldType::U32)]);
        let error = decode_exact(&good, &[1, 2, 3], 165).unwrap_err();
        assert!(error.contains("needs 4 bytes"), "{error}");
    }
}
