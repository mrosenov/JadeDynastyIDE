//! Advanced task search: conditions on any task field (optionally on the same
//! list row) or one value anywhere. Every root is walked from its stored bytes
//! (`visit_task_leaves`, no field trees) on several threads, so a search over a
//! whole task set, unsaved edits included, takes a few seconds at most.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::browser::TaskDocument;
use super::container::TaskContainer;
use super::structures::{AWARD_DATA, TALK};
use super::schema::{is_element_reference, is_task_reference, probe_needed_names, visit_task_leaves, Condition as LayoutCondition, FieldType, LeafPath, LeafValue, NameSet, Schema};

/// Results kept (matches past it are only counted).
pub const LIMIT: usize = 500;
const MATCHES_PER_TASK: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    NotIn,
    Contains,
    Starts,
    Ends,
    HasFlags,
    LacksFlags,
    Empty,
    NotEmpty,
}

impl Op {
    /// Negative conditions must hold for every value of a list field; positive ones for any.
    fn every(self) -> bool {
        matches!(self, Op::Ne | Op::NotIn | Op::LacksFlags | Op::Empty)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCondition {
    /// Field path from the task without indexes (`monsters_wanted.monster_id`).
    pub field: String,
    pub op: Op,
    #[serde(default)]
    pub value: String,
    /// Must hold on the same list row as the condition before it.
    #[serde(default)]
    pub same_row: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum QueryMode {
    #[serde(rename_all = "camelCase")]
    Conditions { conditions: Vec<SearchCondition>, match_all: bool },
    #[serde(rename_all = "camelCase")]
    Value {
        value: String,
        /// Only quest, item, monster and NPC ID fields (and dialog option parameters).
        #[serde(default)]
        references_only: bool,
        #[serde(default)]
        case_sensitive: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Scope {
    All,
    TopLevel,
    /// One quest and everything below it.
    #[serde(rename_all = "camelCase")]
    Under { pack: usize, root: usize, path: Vec<usize> },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    #[serde(flatten)]
    pub mode: QueryMode,
    pub scope: Scope,
}

/// A searchable field of the open layout.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchField {
    /// `success_award.candidates.items.item_id`
    pub path: String,
    /// "int", "float", "bool" or "text".
    pub kind: &'static str,
    /// The lists the field sits in, outermost first (`success_award.candidates`, `…candidates.items`).
    pub arrays: Vec<String>,
    /// "task" or "element" for ID fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<&'static str>,
    /// Inside an award or a dialog: the field that stands for it in every award or dialog
    /// (`any:award:candidates.items.item_id`), usable as a condition field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub any: Option<String>,
}

/// Structures a quest repeats (awards for success, failure and every scale entry; one talk per
/// dialog), with the name their `any:` fields use.
const ANY_ROOTS: [(&str, &str); 2] = [(AWARD_DATA, "award"), (TALK, "dialog")];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    /// Field path with indexes (`monsters_wanted[1].monster_id`).
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub pack: usize,
    pub root: usize,
    pub path: Vec<usize>,
    pub id: u32,
    pub name: String,
    pub matches: Vec<SearchMatch>,
    /// Matching values not listed.
    pub more: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSearchResults {
    pub hits: Vec<SearchHit>,
    /// Matching quests, including those past the limit.
    pub total: usize,
    /// Quests walked; roots that cannot hold a searched ID are skipped without walking.
    pub scanned_tasks: usize,
    pub scanned_roots: usize,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

/// Cancels and reports a running search. A new search or a cancel bumps `generation`.
#[derive(Debug, Default)]
pub struct SearchControl {
    pub generation: AtomicUsize,
    pub scanned: AtomicUsize,
    pub total: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchProgress {
    pub scanned: usize,
    pub total: usize,
}

impl SearchControl {
    pub fn progress(&self) -> SearchProgress {
        SearchProgress { scanned: self.scanned.load(Ordering::Relaxed), total: self.total.load(Ordering::Relaxed) }
    }
}

// ---------------------------------------------------------------- fields

/// Every value field of a task in this layout, without subtasks, raw bytes or pointers.
pub fn field_catalog(schema: &Schema, version: u32) -> Vec<SearchField> {
    fn version_ok(conditions: &[LayoutCondition], version: u32) -> bool {
        conditions.iter().all(|condition| match condition {
            LayoutCondition::Version { min, max } => min.map_or(true, |minimum| version >= minimum) && max.map_or(true, |maximum| version <= maximum),
            LayoutCondition::Field { .. } => true,
        })
    }
    fn leaf(ty: &FieldType) -> Option<&'static str> {
        match ty {
            FieldType::Bool8 => Some("bool"),
            FieldType::I8 | FieldType::U8 | FieldType::I16 | FieldType::U16 | FieldType::I32 | FieldType::U32 | FieldType::I64 | FieldType::U64 => Some("int"),
            FieldType::F32 | FieldType::F64 => Some("float"),
            FieldType::FixedUtf16 { .. } | FieldType::PrefixedUtf16 { .. } | FieldType::CountedUtf16 { .. } => Some("text"),
            _ => None,
        }
    }
    struct Walk<'s> {
        schema: &'s Schema,
        version: u32,
        arrays: Vec<String>,
        /// The innermost award or dialog the walk is in: its `any` name and path.
        any: Option<(&'static str, String)>,
        out: Vec<SearchField>,
        seen: HashMap<String, ()>,
        /// Structures open on the current path: awards hold a selected-role award of their own.
        open: Vec<String>,
    }
    fn walk(state: &mut Walk<'_>, ty: &FieldType, path: String, depth: usize) {
        if depth > 24 {
            return;
        }
        match ty {
            FieldType::Named { name } => {
                let Some(definition) = state.schema.structs.get(name) else { return };
                // A structure nested in itself (an award's selected-role award) is listed one level deep.
                if state.open.iter().filter(|open| *open == name).count() >= 2 {
                    return;
                }
                state.open.push(name.clone());
                let outer = state.any.clone();
                if let Some((_, any)) = ANY_ROOTS.iter().find(|(structure, _)| structure == name) {
                    state.any = Some((any, path.clone()));
                }
                for field in &definition.fields {
                    if !version_ok(&field.when, state.version) || field.name.ends_with("_pointer") {
                        continue;
                    }
                    let child = if path.is_empty() { field.name.clone() } else { format!("{path}.{}", field.name) };
                    walk(state, &field.ty, child, depth + 1);
                }
                state.any = outer;
                state.open.pop();
            }
            FieldType::FixedArray { item, .. } | FieldType::CountedArray { item, .. } => {
                state.arrays.push(path.clone());
                walk(state, item, path, depth + 1);
                state.arrays.pop();
            }
            FieldType::RecursiveArray { .. } => {}
            other => {
                let Some(kind) = leaf(other) else { return };
                if state.seen.insert(path.clone(), ()).is_some() {
                    return;
                }
                let semantic = path.rsplit('.').next().unwrap_or(&path).to_ascii_lowercase();
                let reference = if is_task_reference(&semantic) { Some("task") } else if is_element_reference(&semantic) { Some("element") } else { None };
                let any = state.any.as_ref().and_then(|(name, root)| path.strip_prefix(root.as_str()).and_then(|rest| rest.strip_prefix('.')).map(|rest| format!("any:{name}:{rest}")));
                state.out.push(SearchField { path, kind, arrays: state.arrays.clone(), reference, any });
            }
        }
    }
    let mut state = Walk { schema, version, arrays: Vec::new(), any: None, out: Vec::new(), seen: HashMap::new(), open: Vec::new() };
    walk(&mut state, &FieldType::Named { name: schema.root.clone() }, String::new(), 0);
    state.out
}

/// The last name of a path without its index (`items[2].item_id` → `item_id`, `premise_tasks[3]` → `premise_tasks`).
fn last_name(path: &str) -> &str {
    let last = path.rsplit('.').next().unwrap_or(path);
    last.split('[').next().unwrap_or(last)
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

// ---------------------------------------------------------------- values

fn parse_number(text: &str) -> Option<f64> {
    let text = text.trim();
    match text.to_ascii_lowercase().as_str() {
        "true" | "yes" => return Some(1.0),
        "false" | "no" => return Some(0.0),
        _ => {}
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok().map(|value| value as f64);
    }
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_list(text: &str) -> Vec<f64> {
    text.split(|character: char| character == ',' || character == ';' || character.is_whitespace()).filter(|part| !part.is_empty()).filter_map(parse_number).collect()
}

fn same(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-6 * left.abs().max(right.abs()).max(1.0)
}

/// A value of one leaf, decoded only as far as a test needs.
enum Seen<'b> {
    Int(i128),
    Float(f64),
    Text(&'b [u8]),
}

impl Seen<'_> {
    fn display(&self, kind: Option<&str>) -> String {
        match self {
            Seen::Int(value) if kind == Some("bool") => (if *value != 0 { "true" } else { "false" }).into(),
            Seen::Int(value) => value.to_string(),
            Seen::Float(value) => format!("{value}"),
            Seen::Text(bytes) => utf16(bytes),
        }
    }
}

#[derive(Debug)]
struct Prepared {
    /// As given: a field path or an `any:` field.
    field: String,
    /// The field paths it covers (one, or the same field in every award or dialog).
    paths: Vec<String>,
    /// The part shared by all of `paths` (the path itself, or the part after the award or dialog).
    suffix: String,
    /// Field names between the shared list row and the value, for same-row groups.
    tail: usize,
    op: Op,
    number: Option<f64>,
    numbers: Vec<f64>,
    text: String,
    /// Index of its same-row group.
    group: Option<usize>,
}

impl Prepared {
    fn new(condition: &SearchCondition, kind: &str) -> Result<Self, String> {
        let needs_value = !matches!(condition.op, Op::Empty | Op::NotEmpty);
        let number = parse_number(&condition.value);
        let numbers = parse_list(&condition.value);
        let shown = condition.field.as_str();
        if needs_value && condition.value.trim().is_empty() {
            return Err(format!("{shown}: enter a value"));
        }
        if needs_value && kind != "text" {
            match condition.op {
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::HasFlags | Op::LacksFlags if number.is_none() => return Err(format!("{shown}: {:?} is not a number", condition.value)),
                Op::In | Op::NotIn if numbers.is_empty() => return Err(format!("{shown}: list the numbers, separated by commas")),
                _ => {}
            }
        }
        Ok(Self { field: condition.field.clone(), paths: Vec::new(), suffix: String::new(), tail: 0, op: condition.op, number, numbers, text: condition.value.trim().to_lowercase(), group: None })
    }

    fn test(&self, value: &Seen<'_>) -> Option<bool> {
        let op = self.op;
        match value {
            Seen::Int(_) | Seen::Float(_) => {
                let (number, int) = match value {
                    Seen::Int(value) => (*value as f64, Some(*value)),
                    Seen::Float(value) => (*value, None),
                    Seen::Text(_) => unreachable!(),
                };
                let target = self.number.unwrap_or(0.0);
                Some(match op {
                    Op::Eq => same(number, target),
                    Op::Ne => !same(number, target),
                    Op::Lt => number < target,
                    Op::Le => number <= target,
                    Op::Gt => number > target,
                    Op::Ge => number >= target,
                    Op::In => self.numbers.iter().any(|candidate| same(number, *candidate)),
                    Op::NotIn => !self.numbers.iter().any(|candidate| same(number, *candidate)),
                    Op::HasFlags | Op::LacksFlags => {
                        let (Some(value), Some(mask)) = (int, self.number.map(|mask| mask as i128)) else { return None };
                        if op == Op::HasFlags { value & mask == mask } else { value & mask == 0 }
                    }
                    Op::Contains => value.display(None).contains(&self.text),
                    Op::Starts => value.display(None).starts_with(&self.text),
                    Op::Ends => value.display(None).ends_with(&self.text),
                    Op::Empty => number == 0.0,
                    Op::NotEmpty => number != 0.0,
                })
            }
            Seen::Text(bytes) => {
                let text = utf16(bytes).to_lowercase();
                Some(match op {
                    Op::Eq => text == self.text,
                    Op::Ne => text != self.text,
                    Op::In => self.text.split(',').any(|part| part.trim() == text),
                    Op::NotIn => !self.text.split(',').any(|part| part.trim() == text),
                    Op::Contains => text.contains(&self.text),
                    Op::Starts => text.starts_with(&self.text),
                    Op::Ends => text.ends_with(&self.text),
                    Op::Empty => text.trim().is_empty(),
                    Op::NotEmpty => !text.trim().is_empty(),
                    Op::Lt | Op::Le | Op::Gt | Op::Ge | Op::HasFlags | Op::LacksFlags => return None,
                })
            }
        }
    }
}

/// A prepared query: conditions by the last name of their field, same-row groups and their lists.
struct Plan {
    conditions: Vec<Prepared>,
    /// The last field name of each condition with its conditions (few, so scanned, not hashed).
    by_name: Vec<(String, Vec<usize>)>,
    /// Per group: the list whose rows its conditions share, and its conditions.
    groups: Vec<(String, Vec<usize>)>,
    match_all: bool,
    kinds: HashMap<String, &'static str>,
    value: Option<ValueNeedle>,
    /// Byte pairs a root must contain to possibly match: one of each inner list (see `two_bytes`).
    required: Vec<Vec<[u8; 2]>>,
}

/// The low two little-endian bytes every stored copy of an integer of 256 or more contains,
/// whatever its width, so roots without them cannot hold it.
fn two_bytes(value: f64) -> Option<[u8; 2]> {
    (value.fract() == 0.0 && value.abs() >= 256.0 && value.abs() < 9.0e18).then(|| {
        let bytes = (value as i64).to_le_bytes();
        [bytes[0], bytes[1]]
    })
}

struct ValueNeedle {
    number: Option<f64>,
    text: String,
    references_only: bool,
    case_sensitive: bool,
}

impl Plan {
    fn new(query: &QueryMode, fields: &[SearchField]) -> Result<Self, String> {
        let kinds: HashMap<String, &'static str> = fields.iter().map(|field| (field.path.clone(), field.kind)).collect();
        match query {
            QueryMode::Value { value, references_only, case_sensitive } => {
                let text = value.trim();
                if text.is_empty() {
                    return Err("Enter a value to find".into());
                }
                let number = if text.contains(|character: char| character.is_ascii_digit()) { parse_number(text) } else { None };
                // A float field may hold the number too, so only whole numbers typed without a point narrow the scan.
                let required = number.filter(|_| !text.contains('.')).and_then(two_bytes).map(|pair| vec![vec![pair]]).unwrap_or_default();
                Ok(Self {
                    conditions: Vec::new(),
                    by_name: Vec::new(),
                    groups: Vec::new(),
                    match_all: true,
                    kinds,
                    value: Some(ValueNeedle { number, text: if *case_sensitive { text.to_string() } else { text.to_lowercase() }, references_only: *references_only, case_sensitive: *case_sensitive }),
                    required,
                })
            }
            QueryMode::Conditions { conditions, match_all } => {
                if conditions.is_empty() {
                    return Err("Add a condition".into());
                }
                let mut prepared = Vec::new();
                let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
                // The lists each condition sits in, relative to its suffix.
                let mut lists: Vec<Vec<String>> = Vec::new();
                for (index, condition) in conditions.iter().enumerate() {
                    let covered: Vec<&SearchField> = fields.iter().filter(|field| field.path == condition.field || field.any.as_deref() == Some(condition.field.as_str())).collect();
                    let Some(first) = covered.first() else { return Err(format!("{:?} is not a task field of this layout", condition.field)) };
                    let mut item = Prepared::new(condition, first.kind)?;
                    item.paths = covered.iter().map(|field| field.path.clone()).collect();
                    item.suffix = condition.field.strip_prefix("any:").and_then(|rest| rest.split_once(':')).map_or(condition.field.clone(), |(_, suffix)| suffix.to_string());
                    let root = first.path.len() - item.suffix.len();
                    lists.push(first.arrays.iter().filter(|list| list.len() > root).map(|list| list[root..].to_string()).collect());
                    if condition.same_row && index > 0 {
                        let previous = prepared.len() - 1;
                        let group = match prepared.get(previous).and_then(|entry: &Prepared| entry.group) {
                            Some(group) => group,
                            None => {
                                groups.push((String::new(), vec![previous]));
                                let group = groups.len() - 1;
                                prepared[previous].group = Some(group);
                                group
                            }
                        };
                        groups[group].1.push(prepared.len());
                        item.group = Some(group);
                    }
                    prepared.push(item);
                }
                // Each group shares the innermost list all of its fields sit in; a value's row is its
                // path without the names after that list.
                for (list, members) in &mut groups {
                    let options: Vec<&Vec<String>> = members.iter().map(|member| &lists[*member]).collect();
                    let common = options[0].iter().rev().find(|candidate| options.iter().all(|arrays| arrays.contains(candidate)));
                    *list = common.cloned().ok_or_else(|| {
                        let names = members.iter().map(|member| prepared[*member].field.as_str()).collect::<Vec<_>>().join(", ");
                        format!("\"Same row\" needs fields of one list; {names} share none")
                    })?;
                    for member in members.iter() {
                        prepared[*member].tail = prepared[*member].suffix.split('.').count() - list.split('.').count();
                    }
                }
                let mut by_name: Vec<(String, Vec<usize>)> = Vec::new();
                for (index, condition) in prepared.iter().enumerate() {
                    let name = last_name(&condition.suffix);
                    match by_name.iter_mut().find(|(existing, _)| existing == name) {
                        Some((_, members)) => members.push(index),
                        None => by_name.push((name.to_string(), vec![index])),
                    }
                }
                // With "all", an integer field equal to (or one of) large values needs those bytes in the root.
                let required = if *match_all {
                    prepared.iter().filter(|condition| kinds.get(&condition.paths[0]) == Some(&"int")).filter_map(|condition| match condition.op {
                        Op::Eq => condition.number.and_then(two_bytes).map(|pair| vec![pair]),
                        Op::In => condition.numbers.iter().map(|number| two_bytes(*number)).collect::<Option<Vec<_>>>(),
                        _ => None,
                    }).collect()
                } else {
                    Vec::new()
                };
                Ok(Self { conditions: prepared, by_name, groups, match_all: *match_all, kinds, value: None, required })
            }
        }
    }
}

/// What one task met during the walk.
#[derive(Default)]
struct TaskState {
    /// Positive conditions: a value matched. Negative ones: a value failed.
    hit: Vec<bool>,
    /// Same-row conditions: whether each row satisfies them.
    rows: Vec<HashMap<String, bool>>,
    matches: Vec<SearchMatch>,
    more: usize,
}

impl TaskState {
    fn new(conditions: usize) -> Self {
        Self { hit: vec![false; conditions], rows: (0..conditions).map(|_| HashMap::new()).collect(), matches: Vec::new(), more: 0 }
    }

    fn record(&mut self, field: String, value: String) {
        if self.matches.len() < MATCHES_PER_TASK {
            if !self.matches.iter().any(|existing| existing.field == field) {
                self.matches.push(SearchMatch { field, value });
            }
        } else {
            self.more += 1;
        }
    }
}

impl Plan {
    fn visit(&self, state: &mut TaskState, field: &LeafPath<'_, '_>, value: LeafValue<'_>) {
        let seen = match value {
            LeafValue::Int(value) => Seen::Int(value),
            LeafValue::Float(value) => Seen::Float(value),
            LeafValue::Text(bytes) => Seen::Text(bytes),
            LeafValue::Bytes => return,
        };
        if let Some(needle) = &self.value {
            let name = field.last_name();
            if needle.references_only && !(is_task_reference(name) || is_element_reference(name) || (name == "parameter" && field.has_name("options"))) {
                return;
            }
            let found = match (&seen, needle.number) {
                (Seen::Int(value), Some(number)) => same(*value as f64, number),
                (Seen::Float(value), Some(number)) => needle.text.contains('.') && same(*value, number),
                (Seen::Text(bytes), None) => {
                    let text = utf16(bytes);
                    if needle.case_sensitive { text.contains(&needle.text) } else { text.to_lowercase().contains(&needle.text) }
                }
                _ => false,
            };
            if found {
                state.hit[0] = true;
                state.record(field.indexed(), seen.display(self.kinds.get(&field.plain()).copied()));
            }
            return;
        }
        let name = field.last_name();
        let Some((_, candidates)) = self.by_name.iter().find(|(candidate, _)| candidate == name) else { return };
        let path = field.plain();
        for &index in candidates {
            let condition = &self.conditions[index];
            if !condition.paths.iter().any(|candidate| *candidate == path) {
                continue;
            }
            let Some(ok) = condition.test(&seen) else { continue };
            if condition.group.is_some() {
                let key = field.prefix(field.name_count() - condition.tail);
                let row = state.rows[index].entry(key).or_insert(condition.op.every());
                *row = if condition.op.every() { *row && ok } else { *row || ok };
            } else if condition.op.every() {
                state.hit[index] |= !ok;
            } else {
                state.hit[index] |= ok;
            }
            if ok && (!condition.op.every() || !field.is_indexed()) {
                state.record(field.indexed(), seen.display(self.kinds.get(&path).copied()));
            }
        }
    }

    fn matches(&self, state: &TaskState) -> bool {
        if self.value.is_some() {
            return state.hit[0];
        }
        let mut units = Vec::new();
        for (index, condition) in self.conditions.iter().enumerate() {
            if condition.group.is_none() {
                units.push(if condition.op.every() { !state.hit[index] } else { state.hit[index] });
            }
        }
        for (_, members) in &self.groups {
            let first = &state.rows[members[0]];
            units.push(first.iter().any(|(row, ok)| *ok && members[1..].iter().all(|member| state.rows[*member].get(row).copied().unwrap_or(false))));
        }
        if self.match_all { units.iter().all(|unit| *unit) } else { units.iter().any(|unit| *unit) }
    }
}

// ---------------------------------------------------------------- running

/// What a search reads: the task files plus the unsaved roots, so it can run without the document lock.
pub struct SearchSource {
    container: TaskContainer,
    schema: Schema,
    /// `probe_needed_names` of the schema, built once.
    needed: NameSet,
    version: u32,
    modified: HashMap<(usize, usize), Vec<u8>>,
    added: HashMap<usize, Vec<Vec<u8>>>,
}

impl SearchSource {
    pub fn of(document: &TaskDocument) -> Self {
        Self {
            container: document.container.clone(),
            schema: document.schema.clone(),
            needed: probe_needed_names(&document.schema),
            version: document.container.header.version,
            modified: document.modified.iter().map(|(key, root)| (*key, root.current.clone())).collect(),
            added: document.added_roots.clone(),
        }
    }

    pub fn fields(&self) -> Vec<SearchField> {
        field_catalog(&self.schema, self.version)
    }

    fn root_count(&self, pack: usize) -> usize {
        self.container.packs[pack].root_count() + self.added.get(&pack).map_or(0, Vec::len)
    }
}

pub fn run(source: &SearchSource, query: &TaskQuery, control: &SearchControl, generation: usize) -> Result<TaskSearchResults, String> {
    let started = Instant::now();
    let plan = Plan::new(&query.mode, &source.fields())?;
    let cancelled = || control.generation.load(Ordering::SeqCst) != generation;
    let packs: Vec<usize> = match &query.scope {
        Scope::Under { pack, .. } => vec![*pack],
        _ => (0..source.container.packs.len()).collect(),
    };
    let total_roots = match &query.scope {
        Scope::Under { .. } => 1,
        _ => packs.iter().map(|pack| source.root_count(*pack)).sum(),
    };
    control.scanned.store(0, Ordering::Relaxed);
    control.total.store(total_roots, Ordering::Relaxed);

    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |count| count.get()).min(packs.len().max(1));
    let results: Vec<Result<Vec<(usize, Vec<SearchHit>, usize, usize)>, String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads).map(|_| scope.spawn(|| {
            let mut done = Vec::new();
            loop {
                let slot = next.fetch_add(1, Ordering::Relaxed);
                let Some(&pack) = packs.get(slot) else { break };
                if cancelled() {
                    return Err("Search cancelled".to_string());
                }
                let (hits, tasks, roots) = search_pack(source, &plan, &query.scope, pack, control, &cancelled)?;
                done.push((slot, hits, tasks, roots));
            }
            Ok(done)
        })).collect();
        workers.into_iter().map(|worker| worker.join().unwrap_or_else(|_| Err("A search thread failed".into()))).collect()
    });
    let mut by_slot = Vec::new();
    for result in results {
        by_slot.extend(result?);
    }
    by_slot.sort_by_key(|(slot, ..)| *slot);
    let mut report = TaskSearchResults::default();
    for (_, hits, tasks, roots) in by_slot {
        report.total += hits.len();
        report.scanned_tasks += tasks;
        report.scanned_roots += roots;
        for hit in hits {
            if report.hits.len() < LIMIT {
                report.hits.push(hit);
            }
        }
    }
    report.truncated = report.total > report.hits.len();
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

/// Searches the roots of one pack (or the one root of an `Under` scope): hits, tasks and roots scanned.
fn search_pack(source: &SearchSource, plan: &Plan, scope: &Scope, pack: usize, control: &SearchControl, cancelled: &dyn Fn() -> bool) -> Result<(Vec<SearchHit>, usize, usize), String> {
    let pack_info = &source.container.packs[pack];
    let base = pack_info.root_count();
    let only = match scope {
        Scope::Under { root, .. } => Some(*root),
        _ => None,
    };
    // Read the pack once unless every wanted root is held in memory.
    let needs_file = match only {
        Some(root) => root < base && !source.modified.contains_key(&(pack, root)),
        None => (0..base).any(|root| !source.modified.contains_key(&(pack, root))),
    };
    let data = if needs_file { Some(std::fs::read(pack_info.path()).map_err(|error| format!("{}: {error}", pack_info.path().display()))?) } else { None };
    let mut hits = Vec::new();
    let (mut tasks_seen, mut roots_seen) = (0, 0);
    let roots: Vec<usize> = match only {
        Some(root) => vec![root],
        None => (0..source.root_count(pack)).collect(),
    };
    for (position, root) in roots.into_iter().enumerate() {
        if position % 32 == 0 && cancelled() {
            return Err("Search cancelled".into());
        }
        let bytes: &[u8] = if let Some(current) = source.modified.get(&(pack, root)) {
            current
        } else if root >= base {
            source.added.get(&pack).and_then(|added| added.get(root - base)).ok_or_else(|| format!("Task root {}:{} does not exist", pack + 1, root + 1))?
        } else {
            let range = pack_info.root_range(root)?;
            let data = data.as_deref().ok_or("Task pack was not read")?;
            data.get(range.start as usize..range.end as usize).ok_or_else(|| format!("{}: root {} range is outside the pack", pack_info.path().display(), root + 1))?
        };
        roots_seen += 1;
        control.scanned.fetch_add(1, Ordering::Relaxed);
        if !plan.required.iter().all(|options| bytes.windows(2).any(|pair| options.iter().any(|option| pair == option))) {
            continue;
        }
        let conditions = plan.conditions.len().max(1);
        let mut states: Vec<TaskState> = Vec::new();
        let mut visitor = |task: usize, field: &LeafPath<'_, '_>, value: LeafValue<'_>| {
            while states.len() <= task {
                states.push(TaskState::new(conditions));
            }
            plan.visit(&mut states[task], field, value);
        };
        let tasks = visit_task_leaves(&source.schema, bytes, source.version, &source.needed, &mut visitor).map_err(|error| format!("Task root {}:{}: {error}", pack + 1, root + 1))?;
        tasks_seen += tasks.len();
        for (index, task) in tasks.into_iter().enumerate() {
            let in_scope = match scope {
                Scope::All => true,
                Scope::TopLevel => task.path.is_empty(),
                Scope::Under { path, .. } => task.path.starts_with(path),
            };
            let Some(state) = states.get(index) else { continue };
            if in_scope && plan.matches(state) {
                hits.push(SearchHit { pack, root, path: task.path, id: task.id, name: task.name, matches: state.matches.clone(), more: state.more });
            }
        }
    }
    Ok((hits, tasks_seen, roots_seen))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn conditions(conditions: Vec<SearchCondition>, match_all: bool) -> TaskQuery {
        TaskQuery { mode: QueryMode::Conditions { conditions, match_all }, scope: Scope::All }
    }

    fn condition(field: &str, op: Op, value: &str, same_row: bool) -> SearchCondition {
        SearchCondition { field: field.into(), op, value: value.into(), same_row }
    }

    #[test]
    fn last_names() {
        assert_eq!(last_name("fixed.premise_tasks"), "premise_tasks");
        assert_eq!(last_name("success_award.candidates.items.item_id"), "item_id");
    }

    #[test]
    fn searches_a_real_task_set() {
        let path = r"E:/Games/XtremeJade/element/data/tasks.data";
        if !Path::new(path).is_file() {
            return;
        }
        let document = TaskDocument::open(path).unwrap();
        let source = SearchSource::of(&document);
        let fields = source.fields();
        assert!(fields.iter().any(|field| field.path == "monsters_wanted.monster_id" && field.arrays == ["monsters_wanted"]));
        assert!(fields.iter().any(|field| field.path == "success_award.candidates.items.item_id" && field.reference == Some("element")));
        assert!(!fields.iter().any(|field| field.path.contains("subtasks")));
        let control = SearchControl::default();

        // Every quest has an ID; top-level scope keeps only roots.
        let all = run(&source, &conditions(vec![condition("fixed.id", Op::NotEmpty, "", false)], true), &control, 0).unwrap();
        assert_eq!(all.total, all.scanned_tasks);
        assert!(all.truncated && all.hits.len() == LIMIT);
        let mut top = conditions(vec![condition("fixed.id", Op::NotEmpty, "", false)], true);
        top.scope = Scope::TopLevel;
        assert_eq!(run(&source, &top, &control, 0).unwrap().total, document.summary().roots.len());

        // A monster some quest wants, found by condition and as a value anywhere.
        let wanted = run(&source, &conditions(vec![condition("monsters_wanted.monster_id", Op::NotEmpty, "", false)], true), &control, 0).unwrap();
        let hit = &wanted.hits[0];
        let monster = hit.matches.iter().find(|entry| entry.field.starts_with("monsters_wanted[")).unwrap().value.clone();
        let by_id = run(&source, &conditions(vec![condition("monsters_wanted.monster_id", Op::Eq, &monster, false)], true), &control, 0).unwrap();
        assert!(by_id.hits.iter().any(|entry| entry.id == hit.id));
        let anywhere = run(&source, &TaskQuery { mode: QueryMode::Value { value: monster.clone(), references_only: true, case_sensitive: false }, scope: Scope::All }, &control, 0).unwrap();
        assert!(anywhere.hits.iter().any(|entry| entry.id == hit.id));
        assert!(anywhere.total >= by_id.total);

        // Same row: the monster with its own amount matches; with an amount no row has, nothing does.
        let amount = {
            let node = super::super::schema::decode_exact(&document.schema, &document.current_root(hit.pack, hit.root).unwrap(), document.container.header.version).unwrap();
            let task = super::super::browser::task_at(&node, &hit.path).unwrap();
            let row = task.child("monsters_wanted").unwrap().children().iter().find(|row| format!("{:?}", row.child("monster_id").unwrap().value).contains(&monster)).unwrap();
            match row.child("amount").unwrap().value { super::super::schema::Value::U64(value) => value, _ => panic!("amount type") }
        };
        let same_row = |amount: &str| conditions(vec![condition("monsters_wanted.monster_id", Op::Eq, &monster, false), condition("monsters_wanted.amount", Op::Eq, amount, true)], true);
        assert!(run(&source, &same_row(&amount.to_string()), &control, 0).unwrap().hits.iter().any(|entry| entry.id == hit.id));
        assert!(!run(&source, &same_row("987654321"), &control, 0).unwrap().hits.iter().any(|entry| entry.id == hit.id));
        assert!(Plan::new(&conditions(vec![condition("monsters_wanted.monster_id", Op::Eq, "1", false), condition("items_wanted.item_id", Op::Eq, "1", true)], true).mode, &fields).is_err(), "different lists cannot share a row");

        // Any award: a candidate reward item, also on the same row as its amount.
        assert!(fields.iter().any(|field| field.path == "failure_award.candidates.items.item_id" && field.any.as_deref() == Some("any:award:candidates.items.item_id")));
        let rewarded = run(&source, &conditions(vec![condition("success_award.candidates.items.item_id", Op::NotEmpty, "", false)], true), &control, 0).unwrap();
        let reward = &rewarded.hits[0];
        let item = &reward.matches.iter().find(|entry| entry.field.ends_with(".item_id")).unwrap().value;
        let any = run(&source, &conditions(vec![condition("any:award:candidates.items.item_id", Op::Eq, item, false)], true), &control, 0).unwrap();
        assert!(any.hits.iter().any(|entry| entry.id == reward.id));
        let with_amount = run(&source, &conditions(vec![condition("any:award:candidates.items.item_id", Op::Eq, item, false), condition("any:award:candidates.items.amount", Op::Ge, "0", true)], true), &control, 0).unwrap();
        assert!(with_amount.hits.iter().any(|entry| entry.id == reward.id));
        assert!(fields.iter().filter(|field| field.any.as_deref() == Some("any:dialog:windows.options.parameter")).count() > 1);

        // Under one quest: only that tree.
        let under = TaskQuery { mode: QueryMode::Conditions { conditions: vec![condition("fixed.id", Op::NotEmpty, "", false)], match_all: true }, scope: Scope::Under { pack: hit.pack, root: hit.root, path: Vec::new() } };
        assert!(run(&source, &under, &control, 0).unwrap().hits.iter().all(|entry| entry.pack == hit.pack && entry.root == hit.root));

        // Text: the quest's own name.
        let named = run(&source, &conditions(vec![condition("fixed.name", Op::Eq, &hit.name, false)], true), &control, 0).unwrap();
        assert!(named.hits.iter().any(|entry| entry.id == hit.id));

        // A cancelled search stops.
        control.generation.store(5, Ordering::SeqCst);
        assert!(run(&source, &conditions(vec![condition("fixed.id", Op::NotEmpty, "", false)], true), &control, 0).is_err());
    }
}
