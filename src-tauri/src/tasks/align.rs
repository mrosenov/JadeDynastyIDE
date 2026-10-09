//! Proposes a layout patch for a task version JD IDE cannot read yet, by
//! aligning it with a task set of a supported version.
//!
//! Quests are paired by ID. Each reference quest is decoded with its own
//! (verified) schema, and its values are followed through the newer quest's
//! bytes: a running shift says how far the newer bytes have moved, and a
//! mismatch that is confirmed by the following values re-anchors it. Every
//! shift change between two neighbouring fields of a structure is an event; the
//! events of many quests and structure instances vote on how each structure
//! changed. The result is a list of patch operations against the reference
//! schema: inserted unknown blocks (raw, so their bytes stay untouched), removed
//! fields, shrunk fields, fixed lists with another number of items, and runs of
//! fields that cannot be told apart replaced by one unknown block of the newer size.
//!
//! The events are a first estimate. Each structure is then solved field by field
//! against the bytes (`solve_structure`): from a field that matched in many quests,
//! every way of keeping, removing or shortening the following fields and inserting
//! bytes between them is scored by how many non-zero reference bytes reappear in
//! the paired newer quests. That finds changes the events cannot place (values that
//! repeat, such as `ffffffff`, or regions that are mostly zero) and lists that lost
//! or gained items. Inner structures are solved first; their size changes and the
//! texts' length differences are carried into the structures around them.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use super::container::TaskContainer;
use super::schema::{decode_exact, probe_needed_names, visit_task_leaves, CountWidth, FieldDef, FieldType, LeafPath, LeafValue, Node, PatchOperation, Schema, TextLengthUnit};
use super::schema_for_version;

/// How far a value may have moved before it is considered lost.
const MAX_SHIFT: i64 = 2048;
/// Quests compared across the file, plus quests chosen for rarely used fields (up to `MAX_PAIRS`).
const SAMPLE_PAIRS: usize = 400;
const MAX_PAIRS: usize = 1500;
/// Quests chosen for each field that is rarely non-zero.
const RARE_PICKS: usize = 4;
/// Instances that must agree on a structure boundary before it counts.
const MIN_SUPPORT: usize = 10;
/// Instances that must agree before a run of fields is replaced by an unknown block.
const MIN_SPAN_SUPPORT: usize = 10;
/// Structure instances kept per structure for testing placements.
const MAX_STORED: usize = 800;
/// Share of the reference's non-zero bytes a solved structure must find again.
const MIN_SOLVED_RATE: f64 = 0.8;
/// Widest range of byte shifts a structure is solved over.
const MAX_SOLVE_WIDTH: i64 = 4096;
/// Fixed lists grow (instead of a new field following them) by at least this many items.
const MIN_GROWTH_ITEMS: usize = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignChange {
    pub structure: String,
    /// "insert", "remove", "resize", "array_length" or "unknown_block".
    pub kind: String,
    /// The field added, removed or resized; for an unknown block the fields it replaces.
    pub fields: Vec<String>,
    /// The field the change follows (none: the start of the structure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Bytes of the inserted, resized or replacing field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<usize>,
    /// How many bytes the structure gains (or loses) here.
    pub delta: i64,
    /// Instances that agreed, of those that showed this boundary.
    pub support: usize,
    pub seen: usize,
    /// For "array_length": the reference and the newer number of items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<[usize; 2]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignUnresolved {
    pub structure: String,
    pub after: String,
    pub before: String,
    pub delta: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlignProposal {
    pub target_version: u32,
    pub reference_version: u32,
    pub pairs: usize,
    pub changes: Vec<AlignChange>,
    pub unresolved: Vec<AlignUnresolved>,
    /// The patch operations, against the reference schema, in order.
    pub operations: Vec<PatchOperation>,
    /// Sampled newer quests that decode byte-for-byte with the proposed layout.
    pub sample_exact: usize,
    pub sample_tested: usize,
    pub elapsed_ms: u64,
}

/// A structure instance in a decoded reference quest.
struct Instance {
    structure: String,
    start: usize,
    end: usize,
    /// (field index in the structure definition, first matched shift, last matched shift)
    fields: Vec<(usize, Option<i64>, Option<i64>)>,
    /// Offset and length of each entry of `fields` in the reference quest.
    places: Vec<(usize, usize)>,
}

/// How the newer quest's length of a value is found.
#[derive(Clone, Copy)]
enum LeafKind {
    /// Same length in both files.
    Fixed,
    /// Text with its own length prefix (`prefix` bytes, `unit` bytes per counted unit).
    Prefixed { prefix: usize, unit: usize },
    /// Text whose length is another value of the same structure (`count` is that leaf).
    Counted { count: usize, unit: usize },
}

/// A counted list of a reference instance: its field, count value and the leaf holding the count.
struct CountedList {
    owner: (usize, usize),
    count: usize,
    count_leaf: usize,
}

/// A value of the reference quest and the instance fields it lies in (innermost last).
struct Leaf {
    start: usize,
    len: usize,
    kind: LeafKind,
    /// Text (names, descriptions, dialogs): data that differs between translations.
    text: bool,
    owners: Vec<(usize, usize)>,
}

#[allow(clippy::too_many_arguments)]
fn collect(node: &Node, schema: &Schema, owners: &mut Vec<(usize, usize)>, direct: Option<(usize, usize)>, field_leaves: &mut HashMap<(usize, usize), usize>, instances: &mut Vec<Instance>, leaves: &mut Vec<Leaf>, lists: &mut Vec<CountedList>) {
    if let FieldType::Named { name } = &node.ty {
        let Some(definition) = schema.structs.get(name) else { return };
        let index = instances.len();
        instances.push(Instance { structure: name.clone(), start: node.offset, end: node.offset + node.byte_len, fields: Vec::new(), places: Vec::new() });
        for child in node.children() {
            let Some(field) = definition.fields.iter().position(|candidate| candidate.name == child.name) else { continue };
            instances[index].fields.push((field, None, None));
            instances[index].places.push((child.offset, child.byte_len));
            owners.push((index, instances[index].fields.len() - 1));
            collect(child, schema, owners, Some((index, field)), field_leaves, instances, leaves, lists);
            owners.pop();
        }
        return;
    }
    if let (FieldType::CountedArray { count_field, .. }, Some(owner)) = (&node.ty, owners.last().copied()) {
        if let Some(count_leaf) = counted(direct, count_field, schema, instances, field_leaves) {
            lists.push(CountedList { owner, count: node.children().len(), count_leaf });
        }
    }
    if !node.children().is_empty() {
        for child in node.children() {
            collect(child, schema, owners, None, field_leaves, instances, leaves, lists);
        }
        return;
    }
    if node.byte_len == 0 && !matches!(node.ty, FieldType::CountedUtf16 { .. } | FieldType::CountedBytes { .. }) {
        return;
    }
    let unit_bytes = |unit: &TextLengthUnit| if matches!(unit, TextLengthUnit::Utf16Units) { 2 } else { 1 };
    let kind = match &node.ty {
        FieldType::PrefixedUtf16 { prefix, unit, .. } => LeafKind::Prefixed { prefix: match prefix { CountWidth::U8 => 1, CountWidth::U16 => 2, CountWidth::U32 => 4 }, unit: unit_bytes(unit) },
        FieldType::CountedUtf16 { count_field, unit } => counted(direct, count_field, schema, instances, field_leaves).map_or(LeafKind::Fixed, |count| LeafKind::Counted { count, unit: unit_bytes(unit) }),
        FieldType::CountedBytes { count_field } => counted(direct, count_field, schema, instances, field_leaves).map_or(LeafKind::Fixed, |count| LeafKind::Counted { count, unit: 1 }),
        _ => LeafKind::Fixed,
    };
    if let Some(key) = direct {
        field_leaves.insert(key, leaves.len());
    }
    let text = matches!(node.ty, FieldType::PrefixedUtf16 { .. } | FieldType::CountedUtf16 { .. } | FieldType::FixedUtf16 { .. });
    leaves.push(Leaf { start: node.offset, len: node.byte_len, kind, text, owners: owners.clone() });
}

/// The leaf holding the count of a counted text: a field of the same structure instance.
fn counted(direct: Option<(usize, usize)>, count_field: &str, schema: &Schema, instances: &[Instance], field_leaves: &HashMap<(usize, usize), usize>) -> Option<usize> {
    let (instance, _) = direct?;
    let definition = schema.structs.get(&instances[instance].structure)?;
    let field = definition.fields.iter().position(|candidate| candidate.name == count_field)?;
    field_leaves.get(&(instance, field)).copied()
}

fn distinctive(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes.iter().any(|byte| *byte != 0) && bytes.iter().any(|byte| *byte != 0xFF)
}

fn matches_at(target: &[u8], value: &[u8], at: i64) -> bool {
    at >= 0 && (at as usize).checked_add(value.len()).is_some_and(|end| end <= target.len()) && &target[at as usize..at as usize + value.len()] == value
}

fn read_count(target: &[u8], at: i64, width: usize) -> Option<usize> {
    if at < 0 { return None; }
    let bytes = target.get(at as usize..at as usize + width)?;
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= u64::from(*byte) << (8 * index);
    }
    usize::try_from(value).ok()
}

/// Follows the reference values through the target. Each matched value gets its start and end
/// shift; the end differs from the start when a text has another length in the newer quest, so the
/// following values carry that difference and only layout changes remain between fields.
fn align_pair(reference: &[u8], target: &[u8], leaves: &[Leaf]) -> (Vec<Option<(i64, i64)>>, Vec<i64>) {
    enum Probe<'a> { Strong(&'a [u8]), Weak(&'a [u8]), Variable, None }
    let probes: Vec<Probe> = leaves.iter().map(|leaf| {
        let value = &reference[leaf.start..leaf.start + leaf.len];
        if !matches!(leaf.kind, LeafKind::Fixed) {
            Probe::Variable
        } else if distinctive(value) {
            Probe::Strong(value)
        } else if value.len() == 1 && value[0] != 0 {
            // One-byte values only confirm a position; they are too common to re-anchor on.
            Probe::Weak(value)
        } else {
            Probe::None
        }
    }).collect();
    let strong: Vec<usize> = (0..leaves.len()).filter(|index| matches!(probes[*index], Probe::Strong(_))).collect();
    // The next variable-length value after each leaf: values past it may have moved for other reasons.
    let mut next_variable = vec![usize::MAX; leaves.len() + 1];
    for index in (0..leaves.len()).rev() {
        next_variable[index] = if matches!(probes[index], Probe::Variable) { index } else { next_variable[index + 1] };
    }
    let mut shifts: Vec<Option<(i64, i64)>> = vec![None; leaves.len()];
    // The shift in effect after each leaf, matched or not (to read counts at their newer place).
    let mut after: Vec<i64> = vec![0; leaves.len()];
    let mut shift = 0i64;
    for index in 0..leaves.len() {
        let start = leaves[index].start as i64;
        match probes[index] {
            Probe::None => {}
            Probe::Weak(value) => {
                if matches_at(target, value, start + shift) { shifts[index] = Some((shift, shift)); }
            }
            Probe::Variable => {
                let at = start + shift;
                let length = match leaves[index].kind {
                    LeafKind::Prefixed { prefix, unit } => read_count(target, at, prefix).map(|count| prefix + count * unit),
                    LeafKind::Counted { count, unit } => read_count(target, leaves[count].start as i64 + after[count], leaves[count].len.min(8)).map(|count| count * unit),
                    LeafKind::Fixed => None,
                };
                if let Some(length) = length.filter(|length| *length <= 1 << 20 && at >= 0 && at as usize + length <= target.len()) {
                    let end = shift + length as i64 - leaves[index].len as i64;
                    shifts[index] = Some((shift, end));
                    shift = end;
                }
            }
            Probe::Strong(value) => {
                if matches_at(target, value, start + shift) {
                    shifts[index] = Some((shift, shift));
                } else {
                    let position = strong.partition_point(|candidate| *candidate <= index);
                    let limit = next_variable[index + 1];
                    let ahead: Vec<usize> = strong[position..].iter().take_while(|&&next| next < limit).take(3).copied().collect();
                    let value_at = |leaf: usize| match probes[leaf] { Probe::Strong(value) => value, _ => &[][..] };
                    // A changed value (the next one still matches here, or nothing before the next text
                    // can confirm a move) keeps the shift.
                    if !ahead.is_empty() && !matches_at(target, value_at(ahead[0]), leaves[ahead[0]].start as i64 + shift) {
                        let confirmed = |candidate: i64| -> bool {
                            if !matches_at(target, value, start + candidate) { return false; }
                            if ahead.is_empty() { return true; }
                            let hits = ahead.iter().filter(|&&next| matches_at(target, value_at(next), leaves[next].start as i64 + candidate)).count();
                            hits * 3 >= ahead.len() * 2
                        };
                        for step in 1..=MAX_SHIFT {
                            if confirmed(shift - step) { shift -= step; shifts[index] = Some((shift, shift)); break; }
                            if confirmed(shift + step) { shift += step; shifts[index] = Some((shift, shift)); break; }
                        }
                    }
                }
            }
        }
        after[index] = shift;
    }
    (shifts, after)
}

/// Bytes a type always takes, when fixed.
fn fixed_width(schema: &Schema, ty: &FieldType, depth: usize) -> Option<usize> {
    if depth > 16 { return None; }
    match ty {
        FieldType::I8 | FieldType::U8 | FieldType::Bool8 => Some(1),
        FieldType::I16 | FieldType::U16 => Some(2),
        FieldType::I32 | FieldType::U32 | FieldType::F32 => Some(4),
        FieldType::I64 | FieldType::U64 | FieldType::F64 => Some(8),
        FieldType::FixedUtf16 { units } => Some(units * 2),
        FieldType::Bytes { len } | FieldType::Raw { len } => Some(*len),
        FieldType::FixedArray { len, item } => Some(len * fixed_width(schema, item, depth + 1)?),
        FieldType::Named { name } => {
            let definition = schema.structs.get(name)?;
            definition.fields.iter().map(|field| if field.when.is_empty() { fixed_width(schema, &field.ty, depth + 1) } else { None }).sum()
        }
        _ => None,
    }
}

/// The structure a container field holds (directly or as array items).
fn inner_structure(ty: &FieldType) -> Option<&str> {
    match ty {
        FieldType::Named { name } => Some(name),
        FieldType::FixedArray { item, .. } | FieldType::CountedArray { item, .. } => inner_structure(item),
        FieldType::RecursiveArray { target, .. } => Some(target),
        _ => None,
    }
}

type Events = HashMap<String, HashMap<(usize, usize), HashMap<i64, usize>>>;

/// A structure instance kept for testing placements: its matched positions (position, start
/// shift, end shift) and where each field lies in the reference quest (position, offset, length).
struct Stored {
    pair: usize,
    start: u32,
    end: u32,
    /// Bytes from here on cannot be compared (a list of another length, or a text whose length
    /// could not be read); `u32::MAX` when the whole instance can.
    until: u32,
    marks: Vec<(usize, i64, i64)>,
    places: Vec<(u32, u32, u32)>,
}

/// A structure solved against the bytes: changes after the anchor field, the share of non-zero
/// reference bytes found again, and the instances used.
struct Solved {
    anchor: usize,
    placement: Placement,
    /// Changes found inside nested fixed-size structures that had no evidence of their own.
    inner: Vec<(String, Placement)>,
    rate: f64,
    used: usize,
}

/// Where a field of a flattened structure comes from: its own field, or a field of a nested
/// fixed-size structure (outer position, structure, position inside it).
#[derive(Clone)]
enum Origin {
    Own(usize),
    Inner(usize, String, usize),
}

/// The fields of a structure with nested fixed-size structures that were not solved on their own
/// spelled out, so changes inside them can be found from the outer structure's instances.
fn flatten(schema: &Schema, fields: &[FieldDef], solved: &std::collections::HashSet<String>) -> (Vec<FieldDef>, Vec<Origin>, Vec<Option<Vec<usize>>>) {
    let mut flat = Vec::new();
    let mut origin = Vec::new();
    // For each outer field: the offsets of its spelled-out fields inside it.
    let mut offsets = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let inner = match &field.ty {
            FieldType::Named { name } if !solved.contains(name) && field.when.is_empty() => schema.structs.get(name).filter(|definition| {
                definition.fields.iter().all(|inner| inner.when.is_empty() && inner_structure(&inner.ty).is_none() && fixed_width(schema, &inner.ty, 0).is_some())
            }).map(|definition| (name.clone(), definition)),
            _ => None,
        };
        match inner {
            Some((name, definition)) => {
                let mut at = 0usize;
                let mut list = Vec::new();
                for (position, inner) in definition.fields.iter().enumerate() {
                    flat.push(inner.clone());
                    origin.push(Origin::Inner(index + 1, name.clone(), position + 1));
                    list.push(at);
                    at += fixed_width(schema, &inner.ty, 0).unwrap_or(0);
                }
                offsets.push(Some(list));
            }
            None => {
                flat.push(field.clone());
                origin.push(Origin::Own(index + 1));
                offsets.push(None);
            }
        }
    }
    (flat, origin, offsets)
}

/// Bytes moved at boundaries of one structure: (boundary, bytes; negative = missing).
type Placement = Vec<(usize, i64)>;

/// The width of one item of a fixed list, and its length.
fn fixed_list(schema: &Schema, ty: &FieldType) -> Option<(usize, usize)> {
    match ty {
        FieldType::FixedArray { len, item } => fixed_width(schema, item, 0).filter(|width| *width > 0).map(|width| (width, *len)),
        _ => None,
    }
}

/// The width a field always takes, when it can lose bytes (fixed and unconditional).
fn shrinkable(schema: &Schema, fields: &[FieldDef], boundary: usize) -> Option<usize> {
    let field = fields.get(boundary.checked_sub(1)?)?;
    if !field.when.is_empty() { return None; }
    fixed_width(schema, &field.ty, 0)
}

/// Fields other fields read: counts and condition controllers (by their last name segment).
fn referenced_names(schema: &Schema) -> std::collections::HashSet<String> {
    fn visit(ty: &FieldType, names: &mut std::collections::HashSet<String>) {
        let last = |path: &str| path.rsplit('.').next().unwrap_or(path).to_string();
        match ty {
            FieldType::CountedUtf16 { count_field, .. } | FieldType::CountedBytes { count_field } | FieldType::RecursiveArray { count_field, .. } => { names.insert(last(count_field)); }
            FieldType::CountedArray { count_field, item } => { names.insert(last(count_field)); visit(item, names); }
            FieldType::FixedArray { item, .. } => visit(item, names),
            _ => {}
        }
    }
    let mut names = std::collections::HashSet::new();
    for definition in schema.structs.values() {
        for field in &definition.fields {
            visit(&field.ty, &mut names);
            for condition in &field.when {
                if let super::schema::Condition::Field { field, .. } = condition { names.insert(field.rsplit('.').next().unwrap_or(field).to_string()); }
            }
        }
    }
    names
}

/// Length differences before `offset` of one pair: (offset, running total) sorted by offset.
fn adjustment(table: &[(u32, i64)], offset: usize) -> i64 {
    let index = table.partition_point(|entry| entry.0 as usize <= offset);
    if index == 0 { 0 } else { table[index - 1].1 }
}

/// Solves one structure against the bytes of its stored instances. From the earliest field that
/// matched in many instances (the anchor), every field is kept, removed (fixed and unconditional),
/// or has its fixed list shortened, and bytes can be inserted after any field; each path is scored
/// by twice the non-zero reference bytes found again minus those compared, less a cost per change.
/// Fields holding other structures are solved on their own and are not scored here.
fn solve_structure(schema: &Schema, fields: &[FieldDef], stored: &[Stored], pairs: &[(Vec<u8>, Vec<u8>)], tables: &[Vec<(u32, i64)>], referenced: &std::collections::HashSet<String>, solved: &std::collections::HashSet<String>) -> Option<Solved> {
    // Only what lies before an instance's first problem is compared.
    let count = fields.len();
    let truncated: Vec<Stored> = stored.iter().map(|instance| {
        let fits = |offset: u32, len: u32| offset as u64 + len as u64 <= instance.until as u64;
        let places: Vec<(u32, u32, u32)> = instance.places.iter().copied().filter(|place| fits(place.1, place.2)).collect();
        let marks = instance.marks.iter().copied().filter(|mark| match mark.0 {
            0 => true,
            position if position > count => instance.end <= instance.until,
            position => places.iter().any(|place| place.0 as usize == position),
        }).collect();
        Stored { pair: instance.pair, start: instance.start, end: instance.end, until: instance.until, marks, places }
    }).collect();
    let stored = &truncated[..];
    let (flat, origin, offsets) = flatten(schema, fields, solved);
    if flat.len() == fields.len() {
        return solve_flat(schema, fields, stored, pairs, tables, referenced);
    }
    // Positions of the outer fields among the flattened ones.
    let mut first = vec![0usize; fields.len() + 2];
    for (index, entry) in origin.iter().enumerate().rev() {
        let outer = match entry { Origin::Own(outer) | Origin::Inner(outer, _, _) => *outer };
        first[outer] = index + 1;
    }
    first[fields.len() + 1] = flat.len() + 1;
    let remapped: Vec<Stored> = stored.iter().map(|instance| {
        let mut places = Vec::new();
        for &(position, offset, len) in &instance.places {
            match &offsets[position as usize - 1] {
                Some(list) => for (step, inner) in list.iter().enumerate() {
                    let width = fixed_width(schema, &flat[first[position as usize] - 1 + step].ty, 0).unwrap_or(0) as u32;
                    places.push(((first[position as usize] + step) as u32, offset + *inner as u32, width));
                },
                None => places.push((first[position as usize] as u32, offset, len)),
            }
        }
        // Marks of spelled-out structures are left out (they are not anchors).
        let marks = instance.marks.iter().filter(|mark| mark.0 == 0 || mark.0 > fields.len() || offsets[mark.0 - 1].is_none()).map(|&(position, start, end)| (if position == 0 { 0 } else { first[position] }, start, end)).collect();
        Stored { pair: instance.pair, start: instance.start, end: instance.end, until: instance.until, marks, places }
    }).collect();
    let result = solve_flat(schema, &flat, &remapped, pairs, tables, referenced)?;
    // Back to the outer structure, and to the nested ones (the most common answer of each).
    let mut own = Vec::new();
    let mut nested: BTreeMap<String, BTreeMap<usize, Placement>> = BTreeMap::new();
    for (name, list) in origin.iter().filter_map(|entry| match entry { Origin::Inner(outer, name, _) => Some((name.clone(), *outer)), _ => None }) {
        nested.entry(name).or_default().entry(list).or_default();
    }
    for &(boundary, bytes) in &result.placement {
        if boundary == 0 { own.push((0, bytes)); continue; }
        match &origin[boundary - 1] {
            Origin::Own(outer) => own.push((*outer, bytes)),
            Origin::Inner(outer, name, position) => nested.entry(name.clone()).or_default().entry(*outer).or_default().push((*position, bytes)),
        }
    }
    let anchor = if result.anchor == 0 { 0 } else { match &origin[result.anchor - 1] { Origin::Own(outer) | Origin::Inner(outer, _, _) => *outer } };
    let mut inner = Vec::new();
    for (name, occurrences) in nested {
        let mut votes: Vec<(Placement, usize)> = Vec::new();
        for placement in occurrences.into_values() {
            match votes.iter_mut().find(|vote| vote.0 == placement) { Some(vote) => vote.1 += 1, None => votes.push((placement, 1)) }
        }
        if let Some((placement, _)) = votes.into_iter().max_by_key(|vote| (vote.1, !vote.0.is_empty())) {
            if !placement.is_empty() { inner.push((name, placement)); }
        }
    }
    Some(Solved { anchor, placement: own, inner, rate: result.rate, used: result.used })
}

/// Solves one structure (already flattened) against the bytes; see `solve_structure`.
fn solve_flat(schema: &Schema, fields: &[FieldDef], stored: &[Stored], pairs: &[(Vec<u8>, Vec<u8>)], tables: &[Vec<(u32, i64)>], referenced: &std::collections::HashSet<String>) -> Option<Solved> {
    let count = fields.len();
    let container = |position: usize| inner_structure(&fields[position - 1].ty).is_some();
    let text = |position: usize| matches!(fields[position - 1].ty, FieldType::PrefixedUtf16 { .. } | FieldType::CountedUtf16 { .. } | FieldType::FixedUtf16 { .. });
    // Where an instance starts: known for records at the start of a quest (shift 0), or when its
    // first field matched (nothing can be inserted before a structure's first field).
    let start_shift = |instance: &Stored| -> Option<i64> {
        if instance.start == 0 { return instance.marks.iter().find(|mark| mark.0 == 0).map(|mark| mark.1); }
        instance.marks.iter().find(|mark| mark.0 == 1 && mark.1 == mark.2 && count > 0 && !container(1)).map(|mark| mark.1)
    };
    let mut marked = vec![0usize; count + 1];
    for instance in stored {
        if start_shift(instance).is_some() { marked[0] += 1; }
        for &(position, start, end) in &instance.marks {
            if position > 0 && position <= count && start == end { marked[position] += 1; }
        }
    }
    let need = (stored.len() / 4).max(MIN_SPAN_SUPPORT);
    // A field whose value usually equals a neighbour's (three coefficients of 1.0) can match one
    // field off; it would shift everything by that field's width.
    let ambiguous = |position: usize| -> bool {
        let (mut same, mut seen) = (0usize, 0usize);
        for instance in stored {
            let place = |at: usize| instance.places.iter().find(|place| place.0 as usize == at).map(|place| (place.1 as usize, place.2 as usize));
            let Some((offset, len)) = place(position) else { continue };
            let reference = &pairs[instance.pair].0;
            let value = &reference[offset..offset + len];
            seen += 1;
            let equal = |other: Option<(usize, usize)>| other.is_some_and(|(at, width)| width == len && &reference[at..at + width] == value);
            if equal(position.checked_sub(1).and_then(place)) || equal(place(position + 1)) { same += 1; }
        }
        same * 10 > seen
    };
    let candidate = |position: usize| position == 0 || (!container(position) && !text(position));
    let anchor = (0..=count).find(|&position| marked[position] >= (stored.len() / 10).max(MIN_SPAN_SUPPORT) && candidate(position) && (position == 0 || !ambiguous(position)))
        .or_else(|| (0..=count).find(|&position| marked[position] >= need && candidate(position)))?;
    // Usable instances: the anchor's shift and offset, and the length differences before it.
    let usable: Vec<(&Stored, i64, i64)> = stored.iter().filter_map(|instance| {
        let shift = if anchor == 0 { start_shift(instance)? } else { instance.marks.iter().find(|mark| mark.0 == anchor)?.1 };
        let offset = if anchor == 0 { instance.start as usize } else { instance.places.iter().find(|place| place.0 as usize == anchor)?.1 as usize };
        Some((instance, shift, adjustment(&tables[instance.pair], offset)))
    }).collect();
    // The shifts to consider: those the matched values show, relative to the anchor.
    let mut seen: Vec<i64> = vec![0];
    for (instance, shift, before) in &usable {
        for &(position, start, _) in &instance.marks {
            let offset = if position > count { instance.end as usize } else if let Some(place) = instance.places.iter().find(|place| place.0 as usize == position) { place.1 as usize } else { continue };
            if position != anchor { seen.push(start - shift - (adjustment(&tables[instance.pair], offset) - before)); }
        }
    }
    // Where instances end matters most: it alone shows changes in tails that hold no values.
    let mut ends: Vec<i64> = usable.iter().filter_map(|(instance, shift, before)| {
        let mark = instance.marks.iter().find(|mark| mark.0 == count + 1)?;
        Some(mark.1 - shift - (adjustment(&tables[instance.pair], instance.end as usize) - before))
    }).collect();
    seen.sort_unstable();
    ends.sort_unstable();
    let quantile = |values: &[i64], share: f64| values[((values.len() - 1) as f64 * share) as usize];
    let (mut low, mut high) = (quantile(&seen, 0.02).min(0), quantile(&seen, 0.98).max(0));
    if !ends.is_empty() {
        low = low.min(quantile(&ends, 0.02));
        high = high.max(quantile(&ends, 0.98));
    }
    let (low, high) = (low - 64, high + 64);
    if high - low > MAX_SOLVE_WIDTH { return None; }
    let width = (high - low + 1) as usize;
    let zero = (-low) as usize;
    // Changeable fields: fixed, unconditional, not holding structures; lists by item.
    let changeable = |position: usize| -> Option<(usize, Option<(usize, usize)>)> {
        if container(position) || referenced.contains(&fields[position - 1].name) { return None; }
        let width = shrinkable(schema, fields, position)?;
        Some((width, fixed_list(schema, &fields[position - 1].ty)))
    };
    // Matches of every scored field at every shift (per list item for changeable lists).
    let mut nonzero = vec![0u64; count + 1];
    let mut hits: Vec<Vec<u32>> = vec![Vec::new(); count + 1];
    let mut item_hits: Vec<Vec<u32>> = vec![Vec::new(); count + 1];
    for position in 1..=count {
        if text(position) || position == anchor { continue; }
        hits[position] = vec![0; width];
        if let Some((_, Some((_, len)))) = changeable(position) { item_hits[position] = vec![0; width * len]; }
    }
    for (instance, shift, before) in &usable {
        let (reference, target) = &pairs[instance.pair];
        for &(position, offset, len) in &instance.places {
            let (position, offset, len) = (position as usize, offset as usize, len as usize);
            if position == 0 || position > count || hits[position].is_empty() { continue; }
            let list = changeable(position).and_then(|(_, list)| list);
            let table = &tables[instance.pair];
            for byte in 0..len {
                let value = reference[offset + byte];
                if value == 0 { continue; }
                nonzero[position] += 1;
                // Changes inside nested structures move the bytes after them, byte by byte.
                let moved = adjustment(table, offset + byte) - before;
                let base = (offset + byte) as i64 + shift + moved + low;
                let first = (-base).max(0) as usize;
                let last = ((target.len() as i64 - base).max(0) as usize).min(width);
                if first >= last { continue; }
                let from = (base + first as i64) as usize;
                for (step, found) in target[from..from + (last - first)].iter().enumerate() {
                    if *found == value {
                        let index = first + step;
                        hits[position][index] += 1;
                        if let Some((item, len)) = list { item_hits[position][index * len + byte / item] += 1; }
                    }
                }
            }
        }
    }
    // Instances whose end matched: a small reward for ending on their shift.
    let mut finish = vec![0f64; width];
    for (instance, shift, before) in &usable {
        if let Some(mark) = instance.marks.iter().find(|mark| mark.0 == count + 1) {
            let relative = mark.1 - shift - (adjustment(&tables[instance.pair], instance.end as usize) - before) - low;
            if (0..width as i64).contains(&relative) { finish[relative as usize] += 4.0; }
        }
    }
    let cost = (usable.len() as f64 / 20.0).max(4.0);
    #[derive(Clone, Copy)]
    struct Back { from: u32, kind: u8, amount: u32 }
    const KEEP: u8 = 0;
    const REMOVE: u8 = 1;
    const SHORTEN: u8 = 2;
    const INSERT: u8 = 3;
    let mut steps: Vec<(usize, Vec<Back>)> = Vec::new();
    let mut scores = vec![f64::NEG_INFINITY; width];
    scores[zero] = 0.0;
    let insert = |scores: &[f64], late: f64| -> (Vec<f64>, Vec<Back>) {
        let mut out = scores.to_vec();
        let mut back: Vec<Back> = (0..width).map(|index| Back { from: index as u32, kind: KEEP, amount: 0 }).collect();
        let (mut best, mut best_at) = (f64::NEG_INFINITY, 0usize);
        for index in 0..width {
            if best - cost - 0.02 + late > out[index] {
                out[index] = best - cost - 0.02 + late;
                back[index] = Back { from: best_at as u32, kind: INSERT, amount: (index - best_at) as u32 };
            }
            if scores[index] > best { best = scores[index]; best_at = index; }
        }
        (out, back)
    };
    let (next, back) = insert(&scores, 0.0);
    scores = next;
    steps.push((anchor, back));
    for position in anchor + 1..=count {
        let mut out = vec![f64::NEG_INFINITY; width];
        let mut back = vec![Back { from: 0, kind: KEEP, amount: 0 }; width];
        let scored = !hits[position].is_empty();
        let total = nonzero[position] as f64;
        let option = changeable(position);
        // Without evidence: unknown fields, then pointers and vector capacities (memory addresses
        // that some versions do not store) are removed first, and later fields before earlier ones;
        // inserted bytes go to the earliest place.
        let name = &fields[position - 1].name;
        let late = position as f64 * 1e-6 + if name.starts_with("unknown_") { 1e-3 } else { 0.0 }
            + if name.ends_with("_pointer") || name.ends_with("_pointers") || name.ends_with("_capacity") { 2e-3 } else { 0.0 };
        for index in 0..width {
            let here = scores[index];
            if here == f64::NEG_INFINITY { continue; }
            let kept = if scored { here + 2.0 * f64::from(hits[position][index]) - total } else { here };
            if kept > out[index] { out[index] = kept; back[index] = Back { from: index as u32, kind: KEEP, amount: 0 }; }
            if !scored && option.is_none() { continue; }
            let Some((field_width, list)) = option else { continue };
            if index >= field_width {
                let removed = here - total - cost - 0.01 + late;
                if removed > out[index - field_width] { out[index - field_width] = removed; back[index - field_width] = Back { from: index as u32, kind: REMOVE, amount: field_width as u32 }; }
            }
            if let Some((item, len)) = list {
                let row = &item_hits[position][index * len..index * len + len];
                let mut prefix = vec![0u32; len + 1];
                for (at, value) in row.iter().enumerate() { prefix[at + 1] = prefix[at] + value; }
                for dropped in 1..len {
                    let bytes = dropped * item;
                    if index < bytes { break; }
                    let shortened = here + 2.0 * f64::from(prefix[len - dropped]) - total - cost + late;
                    if shortened > out[index - bytes] { out[index - bytes] = shortened; back[index - bytes] = Back { from: index as u32, kind: SHORTEN, amount: bytes as u32 }; }
                }
            }
        }
        steps.push((position, back));
        let (next, back) = insert(&out, 0.0);
        scores = next;
        steps.push((position, back));
    }
    // Best end, then the changes on the way back.
    let (mut index, _) = scores.iter().enumerate().map(|(index, score)| (index, score + finish[index])).filter(|entry| entry.1.is_finite()).max_by(|a, b| a.1.total_cmp(&b.1))?;
    let mut placement: Placement = Vec::new();
    let (mut matched, mut compared) = (0u64, 0u64);
    for (step, (position, back)) in steps.iter().enumerate().rev() {
        let entry = back[index];
        // Field steps are the odd ones (an insertion step follows each).
        let field_step = step % 2 == 1;
        match entry.kind {
            INSERT => placement.push((*position, i64::from(entry.amount))),
            REMOVE | SHORTEN => placement.push((*position, -i64::from(entry.amount))),
            _ => {}
        }
        if field_step && !hits[*position].is_empty() {
            compared += nonzero[*position];
            let from = entry.from as usize;
            matched += match entry.kind {
                KEEP => u64::from(hits[*position][from]),
                SHORTEN => {
                    let (item, len) = changeable(*position).and_then(|(_, list)| list).unwrap_or((1, 1));
                    let keep = len - entry.amount as usize / item;
                    item_hits[*position][from * len..from * len + keep].iter().map(|value| u64::from(*value)).sum()
                }
                _ => 0,
            };
        }
        index = entry.from as usize;
    }
    // The fields before the anchor, walking back from it: the state is the shift of the field after
    // each boundary; the structure's start is free.
    if anchor > 1 {
        let mut states = vec![f64::NEG_INFINITY; width];
        states[zero] = 0.0;
        // Per boundary: (insertion back-pointers, field back-pointers).
        let mut walk: Vec<(usize, Vec<Back>, Vec<Back>)> = Vec::new();
        for position in (1..anchor).rev() {
            // Bytes inserted after this field: it sits lower than the field after it.
            let mut inserted = states.clone();
            let mut insert_back: Vec<Back> = (0..width).map(|index| Back { from: index as u32, kind: KEEP, amount: 0 }).collect();
            let (mut best, mut best_at) = (f64::NEG_INFINITY, width);
            for index in (0..width).rev() {
                if best_at < width && best - cost - 0.02 > inserted[index] {
                    inserted[index] = best - cost - 0.02;
                    insert_back[index] = Back { from: best_at as u32, kind: INSERT, amount: (best_at - index) as u32 };
                }
                if states[index] > best { best = states[index]; best_at = index; }
            }
            // The field itself: kept, removed or shortened (then it sits higher).
            let mut out = vec![f64::NEG_INFINITY; width];
            let mut field_back = vec![Back { from: 0, kind: KEEP, amount: 0 }; width];
            let scored = !hits[position].is_empty();
            let total = nonzero[position] as f64;
            let option = changeable(position);
            for index in 0..width {
                let here = inserted[index];
                if here == f64::NEG_INFINITY { continue; }
                let kept = if scored { here + 2.0 * f64::from(hits[position][index]) - total } else { here };
                if kept > out[index] { out[index] = kept; field_back[index] = Back { from: index as u32, kind: KEEP, amount: 0 }; }
                let Some((field_width, list)) = option else { continue };
                if index + field_width < width {
                    let removed = here - total - cost - 0.01;
                    if removed > out[index + field_width] { out[index + field_width] = removed; field_back[index + field_width] = Back { from: index as u32, kind: REMOVE, amount: field_width as u32 }; }
                }
                if let Some((item, len)) = list {
                    for dropped in 1..len {
                        let bytes = dropped * item;
                        let at = index + bytes;
                        if at >= width { break; }
                        let row = &item_hits[position][at * len..at * len + len];
                        let kept_hits: u32 = row[..len - dropped].iter().sum();
                        let shortened = here + 2.0 * f64::from(kept_hits) - total - cost;
                        if shortened > out[at] { out[at] = shortened; field_back[at] = Back { from: index as u32, kind: SHORTEN, amount: bytes as u32 }; }
                    }
                }
            }
            walk.push((position, insert_back, field_back));
            states = out;
        }
        if let Some((mut index, _)) = states.iter().enumerate().filter(|entry| entry.1.is_finite()).max_by(|a, b| a.1.total_cmp(b.1)) {
            for (position, insert_back, field_back) in walk.iter().rev() {
                let entry = field_back[index];
                if !hits[*position].is_empty() {
                    compared += nonzero[*position];
                    matched += match entry.kind {
                        KEEP => u64::from(hits[*position][index]),
                        SHORTEN => {
                            let (item, len) = changeable(*position).and_then(|(_, list)| list).unwrap_or((1, 1));
                            let keep = len - entry.amount as usize / item;
                            item_hits[*position][index * len..index * len + keep].iter().map(|value| u64::from(*value)).sum()
                        }
                        _ => 0,
                    };
                }
                if matches!(entry.kind, REMOVE | SHORTEN) { placement.push((*position, -i64::from(entry.amount))); }
                index = entry.from as usize;
                let entry = insert_back[index];
                if entry.kind == INSERT { placement.push((*position, i64::from(entry.amount))); }
                index = entry.from as usize;
            }
        }
    }
    // Inserted bytes before the field change at the same boundary (that field may be removed).
    placement.sort_by_key(|&(boundary, bytes)| (boundary, bytes < 0));
    Some(Solved { anchor, placement, inner: Vec::new(), rate: if compared == 0 { 1.0 } else { matched as f64 / compared as f64 }, used: usable.len() })
}

/// Solves every structure with stored instances, inner structures (smaller instances) first.
fn solve_all(schema: &Schema, stored: &HashMap<String, Vec<Stored>>, pairs: &[(Vec<u8>, Vec<u8>)], adjustments: &[Vec<(u32, i64)>], instance_ends: &[Vec<(u16, u32, Vec<(u16, u32)>)>], ids: &HashMap<String, u16>) -> BTreeMap<String, Solved> {
    let mut adjustments = adjustments.to_vec();
    let mut order: Vec<(&String, f64)> = stored.iter().filter(|(_, list)| !list.is_empty()).map(|(name, list)| {
        let length: f64 = list.iter().map(|instance| instance.places.last().map_or(0.0, |place| f64::from(place.1 + place.2))).sum();
        (name, length / list.len() as f64)
    }).collect();
    order.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(b.0)));
    let mut solved = BTreeMap::new();
    let referenced = referenced_names(schema);
    for (name, _) in order {
        // The task record is mostly nested structures and lists; its own changes come from the events.
        if *name == schema.root { continue; }
        let Some(definition) = schema.structs.get(name) else { continue };
        let tables: Vec<Vec<(u32, i64)>> = adjustments.iter().map(|list| {
            let mut list = list.clone();
            list.sort_by_key(|entry| entry.0);
            let mut total = 0i64;
            list.into_iter().map(|(offset, bytes)| { total += bytes; (offset, total) }).collect()
        }).collect();
        let names: std::collections::HashSet<String> = solved.keys().cloned().collect();
        let Some(result) = solve_structure(schema, &definition.fields, &stored[name], pairs, &tables, &referenced, &names) else { continue };
        if result.rate < MIN_SOLVED_RATE { continue; }
        // Each change moves what follows it in every instance of its structure.
        let mut moved: Vec<(String, Placement)> = vec![(name.clone(), result.placement.clone())];
        for (inner, placement) in &result.inner {
            if solved.contains_key(inner) { continue; }
            moved.push((inner.clone(), placement.clone()));
            solved.insert(inner.clone(), Solved { anchor: 0, placement: placement.clone(), inner: Vec::new(), rate: result.rate, used: result.used });
        }
        for (name, placement) in &moved {
            if placement.is_empty() { continue; }
            let Some(&id) = ids.get(name) else { continue };
            for (pair, instances) in instance_ends.iter().enumerate() {
                for (structure, start, ends) in instances {
                    if *structure != id { continue; }
                    for &(boundary, bytes) in placement {
                        let at = if boundary == 0 { Some(*start) } else { ends.iter().find(|entry| entry.0 as usize == boundary).map(|entry| entry.1) };
                        if let Some(at) = at { adjustments[pair].push((at, bytes)); }
                    }
                }
            }
        }
        solved.insert(name.clone(), result);
    }
    solved
}

/// The patch operations for `delta` bytes at `boundary` (after field `boundary - 1`).
#[allow(clippy::too_many_arguments)]
fn boundary_change(schema: &Schema, structure: &str, fields: &[FieldDef], boundary: usize, delta: i64, support: usize, seen: usize, fresh: &mut dyn FnMut(&str, &Schema) -> String) -> Result<(AlignChange, Vec<PatchOperation>), String> {
    let structure_name = structure.to_string();
    let field = boundary.checked_sub(1).and_then(|index| fields.get(index));
    let list = field.and_then(|field| fixed_list(schema, &field.ty).filter(|_| field.when.is_empty()));
    let resized_list = |field: &FieldDef, items: usize, width: usize| {
        let mut replacement = field.clone();
        if let FieldType::FixedArray { item, .. } = &field.ty {
            replacement.ty = FieldType::FixedArray { len: items, item: item.clone() };
        }
        let old = list.map_or(0, |list| list.1);
        (AlignChange { structure: structure_name.clone(), kind: "array_length".into(), fields: vec![field.name.clone()], after: None, width: Some(width), delta, support, seen, items: Some([old, items]) },
         vec![PatchOperation::Replace { structure: structure_name.clone(), field: field.name.clone(), replacement }])
    };
    if delta > 0 {
        if let (Some(field), Some((item, len))) = (field, list) {
            let extra = delta as usize;
            if extra % item == 0 && extra / item >= MIN_GROWTH_ITEMS {
                return Ok(resized_list(field, len + extra / item, (len + extra / item) * item));
            }
        }
        let after = field.map(|field| field.name.clone());
        let name = fresh(structure, schema);
        return Ok((AlignChange { structure: structure_name.clone(), kind: "insert".into(), fields: vec![name.clone()], after: after.clone(), width: Some(delta as usize), delta, support, seen, items: None },
                   vec![PatchOperation::InsertAfter { structure: structure_name, after, field: FieldDef::new(name, FieldType::Raw { len: delta as usize }) }]));
    }
    let missing = (-delta) as usize;
    let (Some(field), Some(width)) = (field, shrinkable(schema, fields, boundary)) else {
        return Err("bytes are missing where no fixed-width field can lose them".into());
    };
    if let Some((item, len)) = list {
        if missing % item == 0 && missing / item < len {
            return Ok(resized_list(field, len - missing / item, (len - missing / item) * item));
        }
    }
    if missing == width {
        return Ok((AlignChange { structure: structure_name.clone(), kind: "remove".into(), fields: vec![field.name.clone()], after: None, width: None, delta, support, seen, items: None },
                   vec![PatchOperation::Remove { structure: structure_name, field: field.name.clone() }]));
    }
    if missing < width {
        let mut replacement = field.clone();
        replacement.ty = FieldType::Raw { len: width - missing };
        return Ok((AlignChange { structure: structure_name.clone(), kind: "resize".into(), fields: vec![field.name.clone()], after: None, width: Some(width - missing), delta, support, seen, items: None },
                   vec![PatchOperation::Replace { structure: structure_name, field: field.name.clone(), replacement }]));
    }
    Err("more bytes are missing than the field before them holds".into())
}

/// Aligns `target` (any version) with `reference` (a supported version) and proposes a patch.
pub fn propose(target: &Path, reference: &Path) -> Result<AlignProposal, String> {
    let started = Instant::now();
    let target_container = TaskContainer::open(target)?;
    let reference_container = TaskContainer::open(reference)?;
    let reference_version = reference_container.header.version;
    let target_version = target_container.header.version;
    let schema = schema_for_version(reference_version).map_err(|_| format!("The reference must be a supported task set; v{reference_version} is not"))?.frozen_at(reference_version);

    // Reference quests by ID (duplicated IDs are left out).
    let mut by_id: HashMap<u32, Option<(usize, usize)>> = HashMap::new();
    for pack in 0..reference_container.packs.len() {
        let data = std::fs::read(reference_container.packs[pack].path()).map_err(|error| error.to_string())?;
        for root in 0..reference_container.packs[pack].root_count() {
            let range = reference_container.packs[pack].root_range(root)?;
            let id = data.get(range.start as usize..range.start as usize + 4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())).unwrap_or(0);
            by_id.entry(id).and_modify(|entry| *entry = None).or_insert(Some((pack, root)));
        }
    }
    // Newer quests by ID.
    let mut target_by_id: HashMap<u32, Option<(usize, usize)>> = HashMap::new();
    let mut target_order = Vec::new();
    for pack in 0..target_container.packs.len() {
        let data = std::fs::read(target_container.packs[pack].path()).map_err(|error| error.to_string())?;
        for root in 0..target_container.packs[pack].root_count() {
            let range = target_container.packs[pack].root_range(root)?;
            let id = data.get(range.start as usize..range.start as usize + 4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())).unwrap_or(0);
            target_by_id.entry(id).and_modify(|entry| *entry = None).or_insert(Some((pack, root)));
            target_order.push(id);
        }
    }
    let paired = |id: &u32| matches!((by_id.get(id), target_by_id.get(id)), (Some(Some(_)), Some(Some(_))));
    // Quests spread over the file, plus quests where rarely used fields hold a value, so those
    // fields have evidence too (a field that is zero everywhere cannot be located).
    let mut chosen: Vec<u32> = Vec::new();
    let mut taken: std::collections::HashSet<u32> = std::collections::HashSet::new();
    let candidates: Vec<u32> = target_order.iter().copied().filter(|id| paired(id)).collect();
    let step = (candidates.len() / SAMPLE_PAIRS).max(1);
    for id in candidates.iter().step_by(step).take(SAMPLE_PAIRS) {
        if taken.insert(*id) { chosen.push(*id); }
    }
    {
        let needed = probe_needed_names(&schema);
        let mut seen: HashMap<String, usize> = HashMap::new();
        for pack in 0..reference_container.packs.len() {
            let data = std::fs::read(reference_container.packs[pack].path()).map_err(|error| error.to_string())?;
            for root in 0..reference_container.packs[pack].root_count() {
                if chosen.len() >= MAX_PAIRS { break; }
                let range = reference_container.packs[pack].root_range(root)?;
                let bytes = &data[range.start as usize..range.end as usize];
                let id = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
                if taken.contains(&id) || !paired(&id) { continue; }
                let mut wanted = false;
                let mut visitor = |task: usize, field: &LeafPath<'_, '_>, value: LeafValue<'_>| {
                    if task != 0 { return; }
                    let used = match value { LeafValue::Int(value) => value != 0, LeafValue::Float(value) => value != 0.0, _ => false };
                    if used {
                        let count = seen.entry(field.plain()).or_default();
                        if *count < RARE_PICKS { *count += 1; wanted = true; }
                    }
                };
                if visit_task_leaves(&schema, bytes, reference_version, &needed, &mut visitor).is_ok() && wanted && taken.insert(id) {
                    chosen.push(id);
                }
            }
        }
    }
    let mut events: Events = HashMap::new();
    let mut stored: HashMap<String, Vec<Stored>> = HashMap::new();
    let mut pair_bytes: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    // Per pair: texts read with another length (offset after the text, bytes), and where each
    // structure instance ends.
    let mut pair_adjustments: Vec<Vec<(u32, i64)>> = Vec::new();
    let mut instance_ends: Vec<Vec<(u16, u32, Vec<(u16, u32)>)>> = Vec::new();
    let mut structure_ids: HashMap<String, u16> = HashMap::new();
    let mut pairs = 0usize;
    let mut sampled = Vec::new();
    for id in &chosen {
        {
            let (Some(Some((pack, root))), Some(Some((reference_pack, reference_root)))) = (target_by_id.get(id).copied(), by_id.get(id).copied()) else { continue };
            let target_bytes = target_container.root(pack, root)?;
            sampled.push((pack, root));
            let reference_bytes = reference_container.root(reference_pack, reference_root)?;
            let Ok(node) = decode_exact(&schema, &reference_bytes, reference_version) else { continue };
            let mut instances = Vec::new();
            let mut leaves = Vec::new();
            let mut lists = Vec::new();
            collect(&node, &schema, &mut Vec::new(), None, &mut HashMap::new(), &mut instances, &mut leaves, &mut lists);
            let (shifts, after_leaf) = align_pair(&reference_bytes, &target_bytes, &leaves);
            pairs += 1;
            // Start and end shift of every instance field.
            for (leaf, shift) in leaves.iter().zip(&shifts) {
                let Some((start, end)) = shift else { continue };
                for &(instance, field) in &leaf.owners {
                    let entry = &mut instances[instance].fields[field];
                    if entry.1.is_none() { entry.1 = Some(*start); }
                    entry.2 = Some(*end);
                }
            }
            // A list with another number of rows in this newer quest is a data difference: the list and
            // the boundaries around it say nothing about the layout here.
            // Instances containing a list of another length or a text whose length could not be read
            // cannot be compared byte by byte; other texts' length differences are carried along.
            // Instance -> first offset that cannot be compared.
            let mut unclean: HashMap<usize, usize> = HashMap::new();
            let mut mark_unclean = |owners: &[(usize, usize)], offset: usize| for owner in owners {
                let entry = unclean.entry(owner.0).or_insert(offset);
                *entry = (*entry).min(offset);
            };
            for list in &lists {
                let count_leaf = &leaves[list.count_leaf];
                let target_count = read_count(&target_bytes, count_leaf.start as i64 + after_leaf[list.count_leaf], count_leaf.len.min(8));
                if target_count != Some(list.count) {
                    let entry = &mut instances[list.owner.0].fields[list.owner.1];
                    entry.1 = None;
                    entry.2 = None;
                    mark_unclean(&count_leaf.owners, count_leaf.start);
                }
            }
            let mut adjustments = Vec::new();
            for (leaf, shift) in leaves.iter().zip(&shifts) {
                if matches!(leaf.kind, LeafKind::Fixed) { continue; }
                match shift {
                    Some((start, end)) if start != end => adjustments.push(((leaf.start + leaf.len) as u32, end - start)),
                    Some(_) => {}
                    None => mark_unclean(&leaf.owners, leaf.start),
                }
            }
            pair_adjustments.push(adjustments);
            for instance in &instances {
                let next = structure_ids.len() as u16;
                structure_ids.entry(instance.structure.clone()).or_insert(next);
            }
            instance_ends.push(instances.iter().map(|instance| {
                let ends = instance.fields.iter().zip(&instance.places).map(|(field, place)| ((field.0 + 1) as u16, (place.0 + place.1) as u32)).collect();
                (structure_ids[&instance.structure], instance.start as u32, ends)
            }).collect());
            for (instance_index, instance) in instances.iter().enumerate() {
                let Some(definition) = schema.structs.get(&instance.structure) else { continue };
                let count = definition.fields.len();
                let before_index = leaves.partition_point(|leaf| leaf.start < instance.start);
                let after_index = leaves.partition_point(|leaf| leaf.start < instance.end);
                // The start and end only count when the value right beside them matched, so unrelated
                // differences before or after the instance (other text, other lists) do not leak in.
                let before = if instance.start == 0 { Some(0) } else { before_index.checked_sub(1).and_then(|index| shifts[index].filter(|_| leaves[index].start + leaves[index].len == instance.start)).map(|shift| shift.1) };
                let after = shifts.get(after_index).copied().flatten().filter(|_| leaves[after_index].start == instance.end).map(|shift| shift.0);
                // Positions: 0 = start, field k = k + 1, count + 1 = end; (position, start shift, end shift).
                let mut sequence: Vec<(usize, i64, i64)> = Vec::new();
                if let Some(before) = before { sequence.push((0, before, before)); }
                for &(field, first, last) in &instance.fields {
                    if let (Some(first), Some(last)) = (first, last) { sequence.push((field + 1, first, last)); }
                }
                if let Some(after) = after { sequence.push((count + 1, after, after)); }
                let kept = stored.entry(instance.structure.clone()).or_default();
                let until = unclean.get(&instance_index).copied().unwrap_or(u32::MAX as usize);
                // Partly comparable instances only for whole quests (their header and awards come before
                // the texts and dialogs that usually stop the comparison); other structures have enough
                // complete ones.
                let usable = until == u32::MAX as usize || (instance.start == 0 && until > instance.start);
                if kept.len() < MAX_STORED && !sequence.is_empty() && usable {
                    let places = instance.fields.iter().zip(&instance.places).map(|(field, place)| ((field.0 + 1) as u32, place.0 as u32, place.1 as u32)).collect();
                    kept.push(Stored { pair: pair_bytes.len(), start: instance.start as u32, end: instance.end as u32, until: until as u32, marks: sequence.clone(), places });
                }
                // Fields of this instance without a matched value, and whether their size varies.
                let present: Vec<usize> = instance.fields.iter().map(|field| field.0).collect();
                let variable_between = |a: usize, b: usize| present.iter().any(|&field| field + 1 > a && field + 1 < b && instance.fields.iter().any(|entry| entry.0 == field && entry.1.is_none()) && fixed_width(&schema, &definition.fields[field].ty, 0).is_none());
                let structure = events.entry(instance.structure.clone()).or_default();
                for window in sequence.windows(2) {
                    let (a, _, a_end) = window[0];
                    let (b, b_start, _) = window[1];
                    if variable_between(a, b) { continue; }
                    *structure.entry((a, b)).or_default().entry(b_start - a_end).or_default() += 1;
                }
            }
            // Texts are not compared byte by byte (translations differ between servers): blank them in
            // the kept copy, as only non-zero reference bytes are scored.
            let mut reference_bytes = reference_bytes;
            for leaf in leaves.iter().filter(|leaf| leaf.text) {
                reference_bytes[leaf.start..leaf.start + leaf.len].fill(0);
            }
            pair_bytes.push((reference_bytes, target_bytes));
        }
    }

    // How each structure changed. Positions are 0 (start), field k + 1, count + 1 (end); boundary k
    // lies between positions k and k + 1, and an observed (a, b, delta) says the boundaries a..b
    // together gained delta bytes. Short, well-supported observations settle single boundaries first;
    // what is left in a longer one either falls on its one open boundary or stays ambiguous.
    struct Plan {
        /// Bytes gained at each boundary, when known.
        boundaries: Vec<Option<i64>>,
        /// Boundaries whose bytes are only known together: (first, last, bytes, support, seen).
        spans: Vec<(usize, usize, i64, usize, usize)>,
        /// Support of the observation that settled each boundary.
        support: Vec<(usize, usize)>,
    }
    let mut plans: BTreeMap<String, Plan> = BTreeMap::new();
    for (structure, observations) in &events {
        let Some(definition) = schema.structs.get(structure) else { continue };
        let count = definition.fields.len();
        let mut agreed: Vec<(usize, usize, i64, usize, usize)> = Vec::new();
        for (&(a, b), deltas) in observations {
            let seen: usize = deltas.values().sum();
            let Some((&delta, &support)) = deltas.iter().max_by_key(|(delta, support)| (**support, -delta.abs())) else { continue };
            if support >= MIN_SUPPORT && support * 4 >= seen * 3 {
                agreed.push((a, b, delta, support, seen));
            }
        }
        let mut plan = Plan { boundaries: vec![None; count + 1], spans: Vec::new(), support: vec![(0, 0); count + 1] };
        // Pass 1, best-supported first: an observation settles its one open boundary.
        agreed.sort_by_key(|&(a, b, _, support, _)| (std::cmp::Reverse(support), b - a));
        let mut deferred = Vec::new();
        for (a, b, delta, support, seen) in agreed {
            let fixed: i64 = (a..b).filter_map(|boundary| plan.boundaries[boundary]).sum();
            let free: Vec<usize> = (a..b).filter(|boundary| plan.boundaries[*boundary].is_none()).collect();
            let rest = delta - fixed;
            match free.len() {
                0 => {}
                1 => { plan.boundaries[free[0]] = Some(rest); plan.support[free[0]] = (support, seen); }
                _ => deferred.push((a, b, delta, support, seen)),
            }
        }
        // Pass 2, narrowest first: what still cannot be placed becomes a span of its open boundaries.
        deferred.sort_by_key(|&(a, b, _, support, _)| (b - a, std::cmp::Reverse(support)));
        for (a, b, delta, support, seen) in deferred {
            let fixed: i64 = (a..b).filter_map(|boundary| plan.boundaries[boundary]).sum();
            let inside: i64 = plan.spans.iter().filter(|span| span.0 >= a && span.1 < b).map(|span| span.2).sum();
            let covered = |boundary: usize| plan.spans.iter().any(|span| boundary >= span.0 && boundary <= span.1);
            let free: Vec<usize> = (a..b).filter(|boundary| plan.boundaries[*boundary].is_none() && !covered(*boundary)).collect();
            let rest = delta - fixed - inside;
            if rest == 0 {
                for boundary in free { plan.boundaries[boundary] = Some(0); }
                continue;
            }
            match free.len() {
                0 => {}
                1 => { plan.boundaries[free[0]] = Some(rest); plan.support[free[0]] = (support, seen); }
                _ if support >= MIN_SPAN_SUPPORT => plan.spans.push((free[0], *free.last().unwrap(), rest, support, seen)),
                _ => {}
            }
        }
        let changed = plan.boundaries.iter().any(|boundary| boundary.is_some_and(|delta| delta != 0)) || !plan.spans.is_empty();
        if changed {
            plans.insert(structure.clone(), plan);
        }
    }

    // Structures solved against the bytes replace the events. A second round knows where many
    // instances end: where the next instance starts (its start follows from its anchor), which
    // places changes in tails that hold no values.
    let first_round = solve_all(&schema, &stored, &pair_bytes, &pair_adjustments, &instance_ends, &structure_ids);
    let mut starts: HashMap<(usize, u32), i64> = HashMap::new();
    for (name, list) in &stored {
        let anchor = first_round.get(name).map(|result| result.anchor);
        for instance in list {
            let shift = instance.marks.iter().find(|mark| mark.0 == 0).map(|mark| mark.1)
                .or_else(|| anchor.and_then(|anchor| instance.marks.iter().find(|mark| mark.0 == anchor && mark.1 == mark.2).map(|mark| mark.1)));
            if let Some(shift) = shift { starts.entry((instance.pair, instance.start)).or_insert(shift); }
        }
    }
    for (name, list) in stored.iter_mut() {
        let Some(count) = schema.structs.get(name).map(|definition| definition.fields.len()) else { continue };
        for instance in list.iter_mut() {
            if instance.marks.iter().any(|mark| mark.0 == count + 1) { continue; }
            if let Some(&shift) = starts.get(&(instance.pair, instance.end)) { instance.marks.push((count + 1, shift, shift)); }
        }
    }
    let solved = solve_all(&schema, &stored, &pair_bytes, &pair_adjustments, &instance_ends, &structure_ids);
    let limit = |structure: &str| if solved.contains_key(structure) { 0 } else { usize::MAX };
    // Changes per structure; a container whose own structure changed explains its parent's boundary.
    let changed_structures: Vec<String> = {
        let mut names: Vec<String> = plans.iter().filter(|(name, plan)| {
            let limit = limit(name);
            plan.boundaries.iter().enumerate().any(|(boundary, bytes)| boundary < limit && bytes.is_some_and(|bytes| bytes != 0)) || plan.spans.iter().any(|span| span.1 < limit)
        }).map(|(name, _)| name.clone()).collect();
        names.extend(solved.iter().filter(|(_, result)| !result.placement.is_empty()).map(|(name, _)| name.clone()));
        names
    };
    let mut changes = Vec::new();
    let mut unresolved = Vec::new();
    let mut operations = Vec::new();
    // Operations each change added, and the fallback of a span change.
    let mut operation_counts: Vec<usize> = Vec::new();
    let mut fallbacks: HashMap<usize, (AlignChange, Vec<PatchOperation>)> = HashMap::new();
    let mut next_name = 1usize;
    let mut fresh = |structure: &str, schema: &Schema| -> String {
        loop {
            let name = format!("unknown_v{target_version}_{next_name}");
            next_name += 1;
            if !schema.structs.get(structure).is_some_and(|definition| definition.fields.iter().any(|field| field.name == name)) {
                return name;
            }
        }
    };
    let empty = Plan { boundaries: Vec::new(), spans: Vec::new(), support: Vec::new() };
    let names: std::collections::BTreeSet<&String> = plans.keys().chain(solved.keys()).collect();
    for structure in names {
        let plan = plans.get(structure).unwrap_or(&empty);
        let fields = &schema.structs[structure].fields;
        let limit = limit(structure);
        if let Some(result) = solved.get(structure) {
            for &(boundary, delta) in &result.placement {
                match boundary_change(&schema, structure, fields, boundary, delta, result.used, result.used, &mut fresh) {
                    Ok((change, ops)) => {
                        operation_counts.push(ops.len());
                        operations.extend(ops);
                        changes.push(change);
                    }
                    Err(reason) => unresolved.push(AlignUnresolved { structure: structure.clone(), after: fields[boundary.saturating_sub(1).min(fields.len() - 1)].name.clone(), before: fields.get(boundary).map_or("(end)".into(), |field| field.name.clone()), delta, reason }),
                }
            }
        }
        let name_of = |position: usize| -> String { if position == 0 { "(start)".into() } else if position > fields.len() { "(end)".into() } else { fields[position - 1].name.clone() } };
        let spans: Vec<(usize, usize, i64, usize, usize)> = plan.spans.iter().copied().filter(|span| span.1 < limit).collect();
        let in_span = |boundary: usize| spans.iter().any(|span| boundary >= span.0 && boundary <= span.1);
        for boundary in 0..plan.boundaries.len() {
            let Some(delta) = plan.boundaries[boundary] else { continue };
            if delta == 0 || in_span(boundary) || boundary >= limit { continue; }
            let (support, seen) = plan.support[boundary];
            if boundary > 0 && inner_structure(&fields[boundary - 1].ty).is_some_and(|inner| changed_structures.iter().any(|name| name == inner)) {
                continue;
            }
            match boundary_change(&schema, structure, fields, boundary, delta, support, seen, &mut fresh) {
                Ok((change, ops)) => {
                    operation_counts.push(ops.len());
                    operations.extend(ops);
                    changes.push(change);
                }
                Err(reason) => unresolved.push(AlignUnresolved { structure: structure.clone(), after: name_of(boundary), before: name_of(boundary + 1), delta, reason }),
            }
        }
        for &(first, last, rest, support, seen) in &spans {
            // The fields between the first and last open boundary cannot be told apart. First choice:
            // replace them with one unknown block. Fallback (when the layout rejects that, e.g. a count
            // field lies among them): the change right before the next located field, where new fields
            // are usually added.
            let between: Vec<&FieldDef> = fields[first..last.min(fields.len())].iter().collect();
            let after = (first > 0).then(|| fields[first - 1].name.clone());
            let unresolved_here = |reason: &str| AlignUnresolved { structure: structure.clone(), after: name_of(first), before: name_of(last + 1), delta: rest, reason: reason.into() };
            let fallback = {
                let mut ops = Vec::new();
                let edge = last.min(fields.len());
                if rest > 0 {
                    let anchor = (edge > 0).then(|| fields[edge - 1].name.clone());
                    let name = fresh(structure, &schema);
                    ops.push(PatchOperation::InsertAfter { structure: structure.clone(), after: anchor.clone(), field: FieldDef::new(name.clone(), FieldType::Raw { len: rest as usize }) });
                    Some((AlignChange { structure: structure.clone(), kind: "insert".into(), fields: vec![name], after: anchor, width: Some(rest as usize), delta: rest, support, seen, items: None }, ops))
                } else {
                    let need = (-rest) as usize;
                    let mut taken = Vec::new();
                    let mut sum = 0usize;
                    let mut index = edge;
                    while sum < need && index > first {
                        index -= 1;
                        let field = &fields[index];
                        let Some(width) = fixed_width(&schema, &field.ty, 0).filter(|_| field.when.is_empty()) else { break };
                        sum += width;
                        taken.insert(0, field.name.clone());
                    }
                    (sum >= need).then(|| {
                        for name in &taken {
                            ops.push(PatchOperation::Remove { structure: structure.clone(), field: name.clone() });
                        }
                        let anchor = (index > 0).then(|| fields[index - 1].name.clone());
                        if sum > need {
                            let name = fresh(structure, &schema);
                            ops.push(PatchOperation::InsertAfter { structure: structure.clone(), after: anchor.clone(), field: FieldDef::new(name.clone(), FieldType::Raw { len: sum - need }) });
                            let mut names = vec![name];
                            names.extend(taken);
                            (AlignChange { structure: structure.clone(), kind: "unknown_block".into(), fields: names, after: anchor, width: Some(sum - need), delta: rest, support, seen, items: None }, ops)
                        } else {
                            (AlignChange { structure: structure.clone(), kind: "remove".into(), fields: taken, after: anchor, width: None, delta: rest, support, seen, items: None }, ops)
                        }
                    })
                }
            };
            let primary: Result<(AlignChange, Vec<PatchOperation>), &str> = (|| {
                if between.is_empty() { return Err("no field lies where the bytes changed"); }
                if between.iter().any(|field| !field.when.is_empty()) { return Err("the fields in between are conditional"); }
                let gap = between.iter().map(|field| fixed_width(&schema, &field.ty, 0)).sum::<Option<usize>>().ok_or("the fields in between have variable length")?;
                let target_gap = gap as i64 + rest;
                if target_gap < 0 { return Err("more bytes are missing than the fields in between hold"); }
                let replaced: Vec<String> = between.iter().map(|field| field.name.clone()).collect();
                let mut ops: Vec<PatchOperation> = replaced.iter().map(|name| PatchOperation::Remove { structure: structure.clone(), field: name.clone() }).collect();
                if target_gap == 0 {
                    return Ok((AlignChange { structure: structure.clone(), kind: "remove".into(), fields: replaced, after: after.clone(), width: None, delta: rest, support, seen, items: None }, ops));
                }
                let name = fresh(structure, &schema);
                ops.push(PatchOperation::InsertAfter { structure: structure.clone(), after: after.clone(), field: FieldDef::new(name.clone(), FieldType::Raw { len: target_gap as usize }) });
                let mut names = vec![name];
                names.extend(replaced);
                Ok((AlignChange { structure: structure.clone(), kind: "unknown_block".into(), fields: names, after: after.clone(), width: Some(target_gap as usize), delta: rest, support, seen, items: None }, ops))
            })();
            match (primary, fallback) {
                (Ok((change, ops)), fallback) => {
                    if let Some(fallback) = fallback { fallbacks.insert(changes.len(), fallback); }
                    operation_counts.push(ops.len());
                    operations.extend(ops);
                    changes.push(change);
                }
                (Err(_), Some((change, ops))) => {
                    operation_counts.push(ops.len());
                    operations.extend(ops);
                    changes.push(change);
                }
                (Err(reason), None) => unresolved.push(unresolved_here(reason)),
            }
        }
    }

    // Keep only operations the schema accepts, one change at a time (trying its fallback when rejected).
    let mut accepted: Vec<PatchOperation> = Vec::new();
    let mut kept_changes = Vec::new();
    let mut operation_index = 0usize;
    for (index, change) in changes.into_iter().enumerate() {
        let count = operation_counts[index];
        let group: Vec<PatchOperation> = operations[operation_index..operation_index + count].to_vec();
        operation_index += count;
        let mut trial = accepted.clone();
        trial.extend(group.iter().cloned());
        match schema.with_operations(&trial) {
            Ok(_) => {
                accepted = trial;
                kept_changes.push(change);
            }
            Err(error) => {
                if let Some((fallback, ops)) = fallbacks.remove(&index) {
                    let mut trial = accepted.clone();
                    trial.extend(ops);
                    if schema.with_operations(&trial).is_ok() {
                        accepted = trial;
                        kept_changes.push(fallback);
                        continue;
                    }
                }
                unresolved.push(AlignUnresolved { structure: change.structure.clone(), after: change.after.clone().unwrap_or_else(|| "(start)".into()), before: change.fields.join(", "), delta: change.delta, reason: format!("the layout rejects it: {error}") });
            }
        }
    }

    // How many sampled quests the proposal reads exactly (the frozen schema reads any version alike).
    let patched = schema.with_operations(&accepted)?;
    let mut sample_exact = 0;
    for &(pack, root) in &sampled {
        let bytes = target_container.root(pack, root)?;
        if decode_exact(&patched, &bytes, target_version).is_ok_and(|node| node.encode().is_ok_and(|encoded| encoded == bytes)) {
            sample_exact += 1;
        }
    }
    Ok(AlignProposal {
        target_version,
        reference_version,
        pairs,
        changes: kept_changes,
        unresolved,
        operations: accepted,
        sample_exact,
        sample_tested: sampled.len(),
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(target: &str, reference: &str) -> Option<AlignProposal> {
        if !Path::new(target).is_file() || !Path::new(reference).is_file() {
            return None;
        }
        let proposal = propose(Path::new(target), Path::new(reference)).unwrap();
        eprintln!("{target} vs v{}: {} pairs, {} changes, {} unresolved, sample {}/{} exact, {} ms", proposal.reference_version, proposal.pairs, proposal.changes.len(), proposal.unresolved.len(), proposal.sample_exact, proposal.sample_tested, proposal.elapsed_ms);
        for change in &proposal.changes {
            eprintln!("  {} {} {:?} after {:?} width {:?} delta {} ({}/{})", change.structure, change.kind, change.fields, change.after, change.width, change.delta, change.support, change.seen);
        }
        for item in &proposal.unresolved {
            eprintln!("  UNRESOLVED {} {}..{} {}: {}", item.structure, item.after, item.before, item.delta, item.reason);
        }
        Some(proposal)
    }

    #[test]
    fn a_matching_version_needs_no_changes() {
        // v170 (1559 server) reads exactly with the v172 layout.
        let Some(proposal) = run(r"E:/Game Dev/JD/1559/gamed/config/tasks.data", r"E:/Games/ForsakenJD/element/data/tasks.data") else { return };
        assert!(proposal.operations.is_empty());
        assert_eq!(proposal.sample_exact, proposal.sample_tested);
    }

    #[test]
    fn finds_shorter_lists_and_missing_pointers_in_v186() {
        // Jade Dynasty Reborn v186 is older code than HDN v184 despite its number: every quest is
        // 679 bytes smaller (shorter class lists, no stored pointers or vector capacities).
        let Some(proposal) = run(r"E:/Games/Jade Dynasty Reborn/element/data/tasks.data", r"E:/Games/Elite Jade Dynasty - HDN/element/data/tasks.data") else { return };
        let shortened = |field: &str| proposal.changes.iter().any(|change| change.kind == "array_length" && change.fields[0] == field);
        assert!(shortened("life_again_two_occupation") && shortened("life_again_thr_occupation") && shortened("have_fail_items"));
        assert!(proposal.changes.iter().any(|change| change.kind == "remove" && change.fields[0] == "signature_pointer"));
        assert!(proposal.sample_exact * 10 >= proposal.sample_tested * 3, "at least 30% of the sampled quests read exactly");
    }

    #[test]
    fn the_layout_probe_offers_v172_for_v170() {
        let path = std::path::Path::new(r"E:/Game Dev/JD/1559/gamed/config/tasks.data");
        if !path.exists() { return; }
        let probes = super::super::analyze::probe_layouts(path).unwrap();
        assert_eq!(probes[0].version, 172);
        assert!(probes[0].sampled > 0 && probes[0].exact == probes[0].sampled);
        assert!(probes.iter().any(|probe| probe.version == 165 && probe.exact < probe.sampled));
    }

    #[test]
    fn finds_the_v172_changes_from_v165() {
        // v172 = v165 plus kermis after faction and a longer friendship table.
        let Some(proposal) = run(r"E:/Games/ForsakenJD/element/data/tasks.data", r"E:/Games/XtremeJade/element/data/tasks.data") else { return };
        assert!(proposal.changes.iter().any(|change| change.structure.starts_with("TASK_FIXED") && change.delta == 1), "kermis (one byte among the flags) is found");
        assert!(proposal.changes.iter().any(|change| change.structure.starts_with("TASK_FIXED") && change.delta == 64), "the longer friendship table is found");
        assert!(proposal.sample_exact * 10 >= proposal.sample_tested * 9, "the proposal reads most sampled quests");
    }
}
