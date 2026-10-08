//! Coverage analysis for task versions that do not yet have a verified schema.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::path::Path;

use serde::Serialize;

use super::container::{Pack, TaskContainer, INDEX_MAGIC};
use super::schema::{decode_exact, decode_prefix_diagnostic, Condition, FieldType, Node, PatchOperation, Schema, Value};
use super::{closest_schema_version, schema_for_version, supported_versions};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub version: u32,
    pub export_version: u32,
    pub root_count: usize,
    pub pack_count: usize,
    pub size: u64,
    pub supported: bool,
    pub closest_version: u32,
    pub supported_versions: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceVersion {
    pub version: u32,
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCoverage {
    pub pack: usize,
    pub roots: usize,
    pub exact_roots: usize,
    pub trailing_roots: usize,
    pub failed_roots: usize,
    pub bytes: u64,
    pub decoded_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisIssue {
    pub pack: usize,
    pub root: usize,
    pub root_bytes: usize,
    pub offset: usize,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    pub source: SourceInfo,
    pub baseline_version: u32,
    pub exact_roots: usize,
    pub trailing_roots: usize,
    pub failed_roots: usize,
    pub total_bytes: u64,
    pub decoded_bytes: u64,
    pub root_coverage: f64,
    pub byte_coverage: f64,
    pub exact_round_trip: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_issue: Option<AnalysisIssue>,
    pub packs: Vec<PackCoverage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SizePattern {
    pub reference_bytes: u64,
    pub target_bytes: u64,
    pub delta: i64,
    pub count: usize,
    pub example_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdDifference {
    pub id: u32,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdComparisonReport {
    pub target: SourceInfo,
    pub reference: SourceInfo,
    pub target_roots: usize,
    pub reference_roots: usize,
    pub matched_ids: usize,
    pub same_size: usize,
    pub grown: usize,
    pub shrunk: usize,
    pub renamed: usize,
    pub target_only: usize,
    pub reference_only: usize,
    pub duplicate_ids: usize,
    pub difference_count: usize,
    pub differences_truncated: bool,
    pub pattern_count: usize,
    pub patterns_truncated: bool,
    pub size_patterns: Vec<SizePattern>,
    pub differences: Vec<IdDifference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldCandidate {
    pub structure: String,
    pub after_field: String,
    pub width: usize,
    pub score: f64,
    pub supporting_samples: usize,
    pub tested_samples: usize,
    pub min_offset: usize,
    pub max_offset: usize,
    pub type_hints: Vec<String>,
    pub example_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldCandidateReport {
    pub target: SourceInfo,
    pub reference: SourceInfo,
    pub baseline_version: u32,
    pub matched_ids: usize,
    pub sampled_roots: usize,
    pub candidate_count: usize,
    pub candidates_truncated: bool,
    pub candidates: Vec<FieldCandidate>,
}

#[derive(Debug)]
struct PackResult {
    coverage: PackCoverage,
    first_issue: Option<AnalysisIssue>,
}

#[derive(Debug, Clone)]
struct RootIdentity {
    id: u32,
    name: String,
    bytes: u64,
    pack: usize,
    root: usize,
}

#[derive(Debug)]
struct FieldBoundary {
    structure: String,
    after_field: String,
    offset: usize,
}

#[derive(Debug, Default)]
struct CandidateEvidence {
    tested: usize,
    supporting: usize,
    left_sum: f64,
    shifted_sum: f64,
    gain_sum: f64,
    min_offset: usize,
    max_offset: usize,
    values: Vec<Vec<u8>>,
}

pub fn inspect(path: impl AsRef<Path>) -> Result<SourceInfo, String> {
    let container = TaskContainer::open(path)?;
    Ok(source_info(&container))
}

/// Reads only the index magic and version. The UI uses this before the costly
/// pack verification so unsupported files are not hashed twice.
pub fn source_version(path: impl AsRef<Path>) -> Result<SourceVersion, String> {
    use std::io::Read;
    let path = path.as_ref();
    let mut file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header).map_err(|error| format!("{}: task index header is truncated: {error}", path.display()))?;
    let magic = u32::from_le_bytes(header[0..4].try_into().unwrap());
    if magic != INDEX_MAGIC {
        return Err(format!("{}: not a tasks.data index (magic 0x{magic:08x})", path.display()));
    }
    let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    Ok(SourceVersion { version, supported: supported_versions().contains(&version) })
}

pub fn analyze(path: impl AsRef<Path>, baseline_version: u32) -> Result<AnalysisReport, String> {
    let schema = schema_for_version(baseline_version)
        .map_err(|_| format!("v{baseline_version} is not a supported task-analysis baseline"))?;
    analyze_with_schema(path, &schema, baseline_version, baseline_version)
}

pub fn analyze_with_schema(path: impl AsRef<Path>, schema: &super::schema::Schema, decode_version: u32, baseline_version: u32) -> Result<AnalysisReport, String> {
    let container = TaskContainer::open(path)?;
    schema.validate()?;

    let next_pack = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(container.packs.len().max(1));
    let mut by_pack = std::thread::scope(|scope| -> Result<Vec<Option<PackResult>>, String> {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(scope.spawn(|| {
                let mut scanned = Vec::new();
                loop {
                    let pack_index = next_pack.fetch_add(1, Ordering::Relaxed);
                    let Some(pack) = container.packs.get(pack_index) else { break };
                    scanned.push((pack_index, analyze_pack(pack, pack_index, schema, decode_version)));
                }
                scanned
            }));
        }
        let mut results = (0..container.packs.len()).map(|_| None).collect::<Vec<_>>();
        for handle in handles {
            for (pack_index, result) in handle.join().map_err(|_| "Task analysis worker panicked")? {
                results[pack_index] = Some(result?);
            }
        }
        Ok(results)
    })?;

    let mut packs = Vec::with_capacity(container.packs.len());
    let mut first_issue = None;
    let mut exact_roots = 0;
    let mut trailing_roots = 0;
    let mut failed_roots = 0;
    let mut total_bytes = 0;
    let mut decoded_bytes = 0;
    for (pack_index, result) in by_pack.iter_mut().enumerate() {
        let result = result.take().ok_or_else(|| format!("Task pack {} was not analyzed", pack_index + 1))?;
        if first_issue.is_none() { first_issue = result.first_issue; }
        exact_roots += result.coverage.exact_roots;
        trailing_roots += result.coverage.trailing_roots;
        failed_roots += result.coverage.failed_roots;
        total_bytes += result.coverage.bytes;
        decoded_bytes += result.coverage.decoded_bytes;
        packs.push(result.coverage);
    }
    let root_count = exact_roots + trailing_roots + failed_roots;
    Ok(AnalysisReport {
        source: source_info(&container),
        baseline_version,
        exact_roots,
        trailing_roots,
        failed_roots,
        total_bytes,
        decoded_bytes,
        root_coverage: if root_count == 0 { 100.0 } else { exact_roots as f64 * 100.0 / root_count as f64 },
        byte_coverage: if total_bytes == 0 { 100.0 } else { decoded_bytes as f64 * 100.0 / total_bytes as f64 },
        exact_round_trip: root_count == exact_roots,
        first_issue,
        packs,
    })
}

/// Matches top-level task trees by their stable task ID. Root sizes include
/// every nested task, so recurring deltas are useful evidence for fields added
/// to the task structure even when names are still unknown.
pub fn compare_ids(target: impl AsRef<Path>, reference: impl AsRef<Path>) -> Result<IdComparisonReport, String> {
    const MAX_DIFFERENCES: usize = 500;
    const MAX_PATTERNS: usize = 200;
    let target = TaskContainer::open(target)?;
    let reference = TaskContainer::open(reference)?;
    if !supported_versions().contains(&reference.header.version) {
        return Err(format!("The reference task set is v{}, but the reference must use a verified layout ({})", reference.header.version, supported_versions().iter().map(|version| format!("v{version}")).collect::<Vec<_>>().join(", ")));
    }
    let target_roots = root_identities(&target)?;
    let reference_roots = root_identities(&reference)?;
    let mut target_by_id = HashMap::<u32, Vec<&RootIdentity>>::new();
    let mut reference_by_id = HashMap::<u32, Vec<&RootIdentity>>::new();
    for root in &target_roots { target_by_id.entry(root.id).or_default().push(root); }
    for root in &reference_roots { reference_by_id.entry(root.id).or_default().push(root); }

    let mut ids = target_by_id.keys().chain(reference_by_id.keys()).copied().collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    let mut patterns = HashMap::<(u64, u64), SizePattern>::new();
    let mut matched_ids = 0;
    let mut same_size = 0;
    let mut grown = 0;
    let mut shrunk = 0;
    let mut renamed = 0;
    let mut target_only = 0;
    let mut reference_only = 0;
    let mut duplicate_ids = 0;
    let mut all_differences = Vec::new();
    for id in ids {
        let target_matches = target_by_id.get(&id).map(Vec::as_slice).unwrap_or_default();
        let reference_matches = reference_by_id.get(&id).map(Vec::as_slice).unwrap_or_default();
        if target_matches.len() > 1 || reference_matches.len() > 1 {
            duplicate_ids += 1;
            all_differences.push(IdDifference { id, kind: "duplicate".into(), target_name: target_matches.first().map(|root| root.name.clone()), reference_name: reference_matches.first().map(|root| root.name.clone()), target_bytes: target_matches.first().map(|root| root.bytes), reference_bytes: reference_matches.first().map(|root| root.bytes) });
            continue;
        }
        match (target_matches.first(), reference_matches.first()) {
            (Some(target), Some(reference)) => {
                matched_ids += 1;
                let delta = target.bytes as i128 - reference.bytes as i128;
                if delta == 0 { same_size += 1; }
                else if delta > 0 { grown += 1; }
                else { shrunk += 1; }
                let pattern = patterns.entry((reference.bytes, target.bytes)).or_insert_with(|| SizePattern {
                    reference_bytes: reference.bytes,
                    target_bytes: target.bytes,
                    delta: i64::try_from(delta).unwrap_or(if delta < 0 { i64::MIN } else { i64::MAX }),
                    count: 0,
                    example_ids: Vec::new(),
                });
                pattern.count += 1;
                if pattern.example_ids.len() < 5 { pattern.example_ids.push(id); }
                if target.name != reference.name {
                    renamed += 1;
                    all_differences.push(IdDifference { id, kind: "renamed".into(), target_name: Some(target.name.clone()), reference_name: Some(reference.name.clone()), target_bytes: Some(target.bytes), reference_bytes: Some(reference.bytes) });
                }
            }
            (Some(target), None) => {
                target_only += 1;
                all_differences.push(IdDifference { id, kind: "target_only".into(), target_name: Some(target.name.clone()), reference_name: None, target_bytes: Some(target.bytes), reference_bytes: None });
            }
            (None, Some(reference)) => {
                reference_only += 1;
                all_differences.push(IdDifference { id, kind: "reference_only".into(), target_name: None, reference_name: Some(reference.name.clone()), target_bytes: None, reference_bytes: Some(reference.bytes) });
            }
            (None, None) => unreachable!(),
        }
    }
    let difference_count = all_differences.len();
    all_differences.sort_by_key(|row| (match row.kind.as_str() { "duplicate" => 0, "target_only" => 1, "reference_only" => 2, _ => 3 }, row.id));
    all_differences.truncate(MAX_DIFFERENCES);
    let mut size_patterns = patterns.into_values().collect::<Vec<_>>();
    size_patterns.sort_by_key(|pattern| (std::cmp::Reverse(pattern.count), pattern.delta.abs(), pattern.reference_bytes));
    let pattern_count = size_patterns.len();
    size_patterns.truncate(MAX_PATTERNS);
    Ok(IdComparisonReport {
        target: source_info(&target),
        reference: source_info(&reference),
        target_roots: target_roots.len(),
        reference_roots: reference_roots.len(),
        matched_ids,
        same_size,
        grown,
        shrunk,
        renamed,
        target_only,
        reference_only,
        duplicate_ids,
        difference_count,
        differences_truncated: difference_count > MAX_DIFFERENCES,
        pattern_count,
        patterns_truncated: pattern_count > MAX_PATTERNS,
        size_patterns,
        differences: all_differences,
    })
}

/// Ranks possible fixed-width insertions at known schema field boundaries.
/// This is evidence only: it never modifies a schema or task bytes. A sample
/// supports a candidate when bytes before the boundary still align and bytes
/// after it align substantially better after skipping the proposed width.
pub fn score_fixed_fields(target: impl AsRef<Path>, reference: impl AsRef<Path>) -> Result<FieldCandidateReport, String> {
    score_fixed_fields_with_operations(target, reference, &[])
}

/// Scores the next insertion after normalizing fields already accepted into a
/// user patch. The accepted spans are removed only from temporary root copies;
/// the task files and the stored patch are never changed by scoring.
pub fn score_fixed_fields_with_operations(target: impl AsRef<Path>, reference: impl AsRef<Path>, operations: &[PatchOperation]) -> Result<FieldCandidateReport, String> {
    const MAX_ROOTS: usize = 256;
    const MAX_CANDIDATES: usize = 100;
    const WIDTHS: [usize; 6] = [1, 2, 4, 8, 16, 32];
    const LEFT_WINDOW: usize = 64;
    const RIGHT_WINDOW: usize = 512;

    if operations.iter().any(|operation| !matches!(operation, PatchOperation::InsertAfter { .. })) {
        return Err("Iterative candidate scoring currently supports inserted fixed-width fields only".into());
    }

    let target = TaskContainer::open(target)?;
    let reference = TaskContainer::open(reference)?;
    if !supported_versions().contains(&reference.header.version) {
        return Err(format!("The reference task set is v{}, but candidate scoring needs a verified reference layout ({})", reference.header.version, supported_versions().iter().map(|version| format!("v{version}")).collect::<Vec<_>>().join(", ")));
    }
    let schema = schema_for_version(reference.header.version)?;
    schema.validate()?;
    let patched_schema = schema.with_operations(operations)?;
    let inserted = inserted_fields(&schema, &patched_schema)?;
    let target_roots = root_identities(&target)?;
    let reference_roots = root_identities(&reference)?;
    let mut target_by_id = HashMap::<u32, Vec<&RootIdentity>>::new();
    let mut reference_by_id = HashMap::<u32, Vec<&RootIdentity>>::new();
    for root in &target_roots { target_by_id.entry(root.id).or_default().push(root); }
    for root in &reference_roots { reference_by_id.entry(root.id).or_default().push(root); }

    let mut ids = target_by_id.keys().filter(|id| reference_by_id.contains_key(id)).copied().collect::<Vec<_>>();
    ids.sort_unstable();
    let matched_ids = ids.iter().filter(|id| target_by_id[id].len() == 1 && reference_by_id[id].len() == 1).count();
    let mut evidence = HashMap::<(String, String, usize), CandidateEvidence>::new();
    let mut sampled_roots = 0;
    for id in ids {
        let target_matches = &target_by_id[&id];
        let reference_matches = &reference_by_id[&id];
        if target_matches.len() != 1 || reference_matches.len() != 1 { continue; }
        let target_identity = target_matches[0];
        let reference_identity = reference_matches[0];
        if target_identity.bytes <= reference_identity.bytes { continue; }
        let target_bytes = target.root(target_identity.pack, target_identity.root)?;
        let reference_bytes = reference.root(reference_identity.pack, reference_identity.root)?;
        let decoded = match decode_exact(&schema, &reference_bytes, reference.header.version) {
            Ok(decoded) => decoded,
            Err(_) => continue,
        };
        let mut boundaries = Vec::new();
        let mut accepted_spans = Vec::new();
        collect_patched_boundaries(&decoded, &schema, &inserted, target.header.version, &mut accepted_spans, &mut boundaries)?;
        let target_bytes = match normalize_insertions(&target_bytes, &accepted_spans) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        sampled_roots += 1;
        boundaries.sort_by_key(|boundary| boundary.offset);
        let mut tied = 0;
        while tied < boundaries.len() {
            let mut end = tied + 1;
            while end < boundaries.len() && boundaries[end].offset == boundaries[tied].offset { end += 1; }
            boundaries[tied..end].reverse();
            tied = end;
        }
        let mut found_width = [false; WIDTHS.len()];
        for boundary in boundaries {
            for (width_index, width) in WIDTHS.into_iter().enumerate() {
                if found_width[width_index] { continue; }
                if target_bytes.len() < reference_bytes.len().saturating_add(width) { continue; }
                let position = boundary.offset;
                if position > reference_bytes.len() || position.saturating_add(width) > target_bytes.len() { continue; }
                let left_start = position.saturating_sub(LEFT_WINDOW);
                let left_len = position - left_start;
                let right_len = RIGHT_WINDOW.min(reference_bytes.len() - position).min(target_bytes.len() - position - width);
                if left_len < 16 || right_len < 16 { continue; }
                let left = similarity(&reference_bytes[left_start..position], &target_bytes[left_start..position]);
                let shifted = similarity(&reference_bytes[position..position + right_len], &target_bytes[position + width..position + width + right_len]);
                let direct = similarity(&reference_bytes[position..position + right_len], &target_bytes[position..position + right_len]);
                let shifted_run = matching_prefix(&reference_bytes[position..position + right_len], &target_bytes[position + width..position + width + right_len]);
                let direct_run = matching_prefix(&reference_bytes[position..position + right_len], &target_bytes[position..position + right_len]);
                let entry = evidence.entry((boundary.structure.clone(), boundary.after_field.clone(), width)).or_default();
                entry.tested += 1;
                let gain = shifted - direct;
                let run_supports_shift = shifted_run >= 32 && shifted_run >= direct_run.saturating_add(16);
                if left >= 0.70 && shifted >= 0.70 && (gain >= 0.05 || run_supports_shift) {
                    found_width[width_index] = true;
                    entry.supporting += 1;
                    entry.left_sum += left;
                    entry.shifted_sum += shifted;
                    entry.gain_sum += gain;
                    if entry.supporting == 1 {
                        entry.min_offset = position;
                        entry.max_offset = position;
                    } else {
                        entry.min_offset = entry.min_offset.min(position);
                        entry.max_offset = entry.max_offset.max(position);
                    }
                    let value = target_bytes[position..position + width].to_vec();
                    if entry.values.len() < 5 && !entry.values.contains(&value) { entry.values.push(value); }
                }
            }
        }
        if sampled_roots >= MAX_ROOTS { break; }
    }

    let minimum_support = if sampled_roots >= 3 { 2 } else { 1 };
    let mut candidates = evidence.into_iter().filter_map(|((structure, after_field, width), evidence)| {
        if evidence.supporting < minimum_support { return None; }
        let support_ratio = evidence.supporting as f64 / evidence.tested.max(1) as f64;
        let alignment = (evidence.left_sum + evidence.shifted_sum) / (2.0 * evidence.supporting as f64);
        let gain = evidence.gain_sum / evidence.supporting as f64;
        let score = 100.0 * (support_ratio * 0.45 + alignment * 0.40 + (gain / 0.75).min(1.0) * 0.15);
        Some(FieldCandidate {
            structure,
            after_field,
            width,
            score,
            supporting_samples: evidence.supporting,
            tested_samples: evidence.tested,
            min_offset: evidence.min_offset,
            max_offset: evidence.max_offset,
            type_hints: type_hints(width, &evidence.values),
            example_values: evidence.values.iter().map(|value| value.iter().map(|byte| format!("{byte:02X}")).collect::<Vec<_>>().join(" ")).collect(),
        })
    }).collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.score.total_cmp(&left.score)
        .then_with(|| right.supporting_samples.cmp(&left.supporting_samples))
        .then_with(|| left.structure.cmp(&right.structure))
        .then_with(|| left.after_field.cmp(&right.after_field))
        .then_with(|| left.width.cmp(&right.width)));
    let candidate_count = candidates.len();
    candidates.truncate(MAX_CANDIDATES);
    Ok(FieldCandidateReport {
        target: source_info(&target),
        reference: source_info(&reference),
        baseline_version: reference.header.version,
        matched_ids,
        sampled_roots,
        candidate_count,
        candidates_truncated: candidate_count > MAX_CANDIDATES,
        candidates,
    })
}

#[derive(Debug, Clone)]
struct InsertedField {
    name: String,
    slot: usize,
    width: usize,
    conditions: Vec<Condition>,
}

#[derive(Debug, Clone, Copy)]
struct AcceptedSpan {
    offset: usize,
    width: usize,
}

fn inserted_fields(base: &Schema, patched: &Schema) -> Result<HashMap<String, Vec<InsertedField>>, String> {
    let mut result = HashMap::new();
    for (structure, patched_definition) in &patched.structs {
        let base_definition = base.structs.get(structure).ok_or_else(|| format!("Patched task layout introduced unsupported structure {structure:?}"))?;
        let base_indexes = base_definition.fields.iter().enumerate().map(|(index, field)| (field.name.as_str(), index)).collect::<HashMap<_, _>>();
        let mut slot = 0;
        let mut fields = Vec::new();
        for field in &patched_definition.fields {
            if let Some(index) = base_indexes.get(field.name.as_str()) {
                slot = index + 1;
                continue;
            }
            let width = fixed_width(&field.ty).ok_or_else(|| format!("Iterative scoring needs a fixed-width inserted field, but {structure}.{} is not fixed-width", field.name))?;
            fields.push(InsertedField { name: field.name.clone(), slot, width, conditions: field.when.clone() });
        }
        if !fields.is_empty() { result.insert(structure.clone(), fields); }
    }
    Ok(result)
}

fn fixed_width(ty: &FieldType) -> Option<usize> {
    match ty {
        FieldType::I8 | FieldType::U8 | FieldType::Bool8 => Some(1),
        FieldType::I16 | FieldType::U16 => Some(2),
        FieldType::I32 | FieldType::U32 | FieldType::F32 => Some(4),
        FieldType::I64 | FieldType::U64 | FieldType::F64 => Some(8),
        FieldType::Bytes { len } | FieldType::Raw { len } => Some(*len),
        _ => None,
    }
}

fn collect_patched_boundaries(node: &Node, schema: &Schema, inserted: &HashMap<String, Vec<InsertedField>>, version: u32, spans: &mut Vec<AcceptedSpan>, boundaries: &mut Vec<FieldBoundary>) -> Result<(), String> {
    match &node.value {
        Value::Struct(children) => {
            let structure = match &node.ty {
                FieldType::Named { name } => name,
                _ => &node.name,
            };
            let definition = schema.structs.get(structure).ok_or_else(|| format!("Decoded task structure {structure:?} is missing from its baseline schema"))?;
            let accepted = inserted.get(structure);
            let scope = numeric_scope(children);
            let mut offset = node.offset;
            for slot in 0..=definition.fields.len() {
                if let Some(fields) = accepted {
                    for field in fields.iter().filter(|field| field.slot == slot) {
                        if !inserted_conditions_match(field, version, &scope, structure)? { continue; }
                        spans.push(AcceptedSpan { offset, width: field.width });
                        boundaries.push(FieldBoundary { structure: structure.clone(), after_field: field.name.clone(), offset });
                    }
                }
                if slot == definition.fields.len() { break; }
                let field = &definition.fields[slot];
                if let Some(child) = children.iter().find(|child| child.name == field.name) {
                    boundaries.push(FieldBoundary { structure: structure.clone(), after_field: child.name.clone(), offset: child.offset + child.byte_len });
                    collect_patched_boundaries(child, schema, inserted, version, spans, boundaries)?;
                    offset = child.offset + child.byte_len;
                }
            }
        }
        Value::Array(children) => {
            for child in children { collect_patched_boundaries(child, schema, inserted, version, spans, boundaries)?; }
        }
        _ => {}
    }
    Ok(())
}

fn numeric_scope(children: &[Node]) -> HashMap<String, i128> {
    let mut scope = HashMap::new();
    for child in children {
        collect_numeric_values(child, &child.name, &mut scope);
    }
    scope
}

fn collect_numeric_values(node: &Node, path: &str, scope: &mut HashMap<String, i128>) {
    let number = match &node.value {
        Value::I64(value) => Some(*value as i128),
        Value::U64(value) => Some(*value as i128),
        Value::Bool(value) => Some(i128::from(*value)),
        _ => None,
    };
    if let Some(number) = number {
        scope.insert(path.into(), number);
    }
    if let Value::Struct(children) = &node.value {
        for child in children {
            collect_numeric_values(child, &format!("{path}.{}", child.name), scope);
        }
    }
}

fn inserted_conditions_match(field: &InsertedField, version: u32, scope: &HashMap<String, i128>, structure: &str) -> Result<bool, String> {
    for condition in &field.conditions {
        let matches = match condition {
            Condition::Version { min, max } => min.map_or(true, |minimum| version >= minimum) && max.map_or(true, |maximum| version <= maximum),
            Condition::Field { field: controller, predicate } => {
                let value = scope.get(controller).ok_or_else(|| format!("{structure}.{}: condition field {controller:?} was not decoded in the reference root", field.name))?;
                predicate.matches(*value)
            }
        };
        if !matches { return Ok(false); }
    }
    Ok(true)
}

fn normalize_insertions(target: &[u8], spans: &[AcceptedSpan]) -> Result<Vec<u8>, String> {
    let removed_bytes = spans.iter().try_fold(0usize, |total, span| total.checked_add(span.width).ok_or("Accepted task fields are too large"))?;
    let capacity = target.len().checked_sub(removed_bytes).ok_or("The newer root is shorter than its accepted task fields")?;
    let mut normalized = Vec::with_capacity(capacity);
    let mut source = 0;
    let mut removed = 0usize;
    for span in spans {
        let start = span.offset.checked_add(removed).ok_or("Accepted task field offset overflow")?;
        let end = start.checked_add(span.width).ok_or("Accepted task field width overflow")?;
        if start < source || end > target.len() {
            return Err("Accepted task field is outside the newer root".into());
        }
        normalized.extend_from_slice(&target[source..start]);
        source = end;
        removed += span.width;
    }
    normalized.extend_from_slice(&target[source..]);
    Ok(normalized)
}

fn source_info(container: &TaskContainer) -> SourceInfo {
    SourceInfo {
        path: container.index_path().display().to_string(),
        version: container.header.version,
        export_version: container.header.export_version,
        root_count: container.header.root_count as usize,
        pack_count: container.packs.len(),
        size: std::fs::metadata(container.index_path()).map(|metadata| metadata.len()).unwrap_or(0) + container.total_pack_bytes(),
        supported: supported_versions().contains(&container.header.version),
        closest_version: closest_schema_version(container.header.version),
        supported_versions: supported_versions().to_vec(),
    }
}

fn root_identities(container: &TaskContainer) -> Result<Vec<RootIdentity>, String> {
    const HEADING_BYTES: usize = 64;
    let mut roots = Vec::with_capacity(container.header.root_count as usize);
    for (pack_index, pack) in container.packs.iter().enumerate() {
        let data = std::fs::read(pack.path()).map_err(|error| format!("{}: {error}", pack.path().display()))?;
        for root in 0..pack.root_count() {
            let range = pack.root_range(root)?;
            let start = usize::try_from(range.start).map_err(|_| format!("{}: root {} offset is too large", pack.path().display(), root + 1))?;
            let end = usize::try_from(range.end).map_err(|_| format!("{}: root {} end is too large", pack.path().display(), root + 1))?;
            let bytes = data.get(start..end).ok_or_else(|| format!("{}: root {} range is outside the pack", pack.path().display(), root + 1))?;
            if bytes.len() < HEADING_BYTES {
                return Err(format!("{}: root {} is too short to contain its stable ID and name", pack.path().display(), root + 1));
            }
            roots.push(RootIdentity {
                id: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
                name: fixed_utf16(&bytes[4..HEADING_BYTES]),
                bytes: bytes.len() as u64,
                pack: pack_index,
                root,
            });
        }
    }
    Ok(roots)
}

fn similarity(left: &[u8], right: &[u8]) -> f64 {
    if left.is_empty() { return 0.0; }
    left.iter().zip(right).filter(|(left, right)| left == right).count() as f64 / left.len() as f64
}

fn matching_prefix(left: &[u8], right: &[u8]) -> usize {
    left.iter().zip(right).take_while(|(left, right)| left == right).count()
}

fn type_hints(width: usize, values: &[Vec<u8>]) -> Vec<String> {
    match width {
        1 if values.iter().all(|value| matches!(value.as_slice(), [0] | [1])) => vec!["bool8".into(), "uint8".into(), "raw8".into()],
        1 => vec!["uint8".into(), "int8".into(), "raw8".into()],
        2 => vec!["uint16".into(), "int16".into(), "raw16".into()],
        4 => {
            let plausible_float = !values.is_empty() && values.iter().all(|value| {
                let number = f32::from_le_bytes(value.as_slice().try_into().unwrap());
                number == 0.0 || number.is_finite() && number.abs() >= 1.0e-6 && number.abs() <= 1.0e9
            });
            let mut hints = vec!["uint32".into(), "int32".into()];
            if plausible_float { hints.push("float32".into()); }
            hints.push("raw32".into());
            hints
        }
        8 => vec!["uint64".into(), "int64".into(), "float64".into(), "raw64".into()],
        width => vec![format!("bytes[{width}]"), format!("raw[{width}]")],
    }
}

fn fixed_utf16(bytes: &[u8]) -> String {
    let units = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes(pair.try_into().unwrap())).collect::<Vec<_>>();
    let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

fn analyze_pack(pack: &Pack, pack_index: usize, schema: &super::schema::Schema, version: u32) -> Result<PackResult, String> {
    let data = std::fs::read(pack.path()).map_err(|error| format!("{}: {error}", pack.path().display()))?;
    let mut coverage = PackCoverage {
        pack: pack_index,
        roots: pack.root_count(),
        exact_roots: 0,
        trailing_roots: 0,
        failed_roots: 0,
        bytes: 0,
        decoded_bytes: 0,
    };
    let mut first_issue = None;
    for root in 0..pack.root_count() {
        let range = pack.root_range(root)?;
        let start = usize::try_from(range.start).map_err(|_| format!("{}: root {} offset is too large", pack.path().display(), root + 1))?;
        let end = usize::try_from(range.end).map_err(|_| format!("{}: root {} end is too large", pack.path().display(), root + 1))?;
        let bytes = data.get(start..end).ok_or_else(|| format!("{}: root {} range is outside the pack", pack.path().display(), root + 1))?;
        coverage.bytes += bytes.len() as u64;
        match decode_prefix_diagnostic(schema, bytes, version) {
            Ok((node, used)) => match node.encode() {
                Ok(encoded) if encoded == bytes[..used] && used == bytes.len() => {
                    coverage.exact_roots += 1;
                    coverage.decoded_bytes += used as u64;
                }
                Ok(encoded) if encoded == bytes[..used] => {
                    coverage.trailing_roots += 1;
                    coverage.decoded_bytes += used as u64;
                    first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), used, "trailing", format!("{} trailing bytes begin at offset 0x{used:X}", bytes.len() - used)));
                }
                Ok(_) => {
                    coverage.failed_roots += 1;
                    coverage.decoded_bytes += used as u64;
                    first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), used, "round_trip", "The baseline decoded this prefix, but encoding it did not reproduce the original bytes"));
                }
                Err(message) => {
                    coverage.failed_roots += 1;
                    coverage.decoded_bytes += used as u64;
                    first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), used, "round_trip", message));
                }
            },
            Err(failure) => {
                coverage.failed_roots += 1;
                coverage.decoded_bytes += failure.offset.min(bytes.len()) as u64;
                first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), failure.offset, "failed", failure.message));
            }
        }
    }
    Ok(PackResult { coverage, first_issue })
}

fn issue(pack: usize, root: usize, root_bytes: usize, offset: usize, kind: impl Into<String>, message: impl Into<String>) -> AnalysisIssue {
    AnalysisIssue { pack, root, root_bytes, offset, kind: kind.into(), message: message.into() }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use md5::{Digest, Md5};

    use super::*;

    fn single_root_set(root: &[u8], version: u32, export_version: u32, label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let folder = std::env::temp_dir().join(format!("jdide-task-{label}-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&folder).unwrap();
        let index = folder.join("tasks.data");
        let pack = folder.join("tasks.data1");
        let mut pack_bytes = Vec::new();
        pack_bytes.extend_from_slice(&0x0693_4554u32.to_le_bytes());
        pack_bytes.extend_from_slice(&1u32.to_le_bytes());
        pack_bytes.extend_from_slice(&12u32.to_le_bytes());
        pack_bytes.extend_from_slice(root);
        std::fs::write(&pack, &pack_bytes).unwrap();
        let digest: [u8; 16] = Md5::digest(&pack_bytes).into();
        let mut index_bytes = Vec::new();
        index_bytes.extend_from_slice(&0x6934_0304u32.to_le_bytes());
        index_bytes.extend_from_slice(&version.to_le_bytes());
        index_bytes.extend_from_slice(&export_version.to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&digest);
        std::fs::write(&index, index_bytes).unwrap();
        (folder, index)
    }

    #[test]
    fn unchanged_newer_header_scores_the_matching_baseline_exactly() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        let source = TaskContainer::open(source_path).unwrap();
        let root = source.root(0, 0).unwrap();
        let folder = std::env::temp_dir().join(format!("jdide-task-analyze-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&folder).unwrap();
        let index = folder.join("tasks.data");
        let pack = folder.join("tasks.data1");
        let mut pack_bytes = Vec::new();
        pack_bytes.extend_from_slice(&0x0693_4554u32.to_le_bytes());
        pack_bytes.extend_from_slice(&1u32.to_le_bytes());
        pack_bytes.extend_from_slice(&12u32.to_le_bytes());
        pack_bytes.extend_from_slice(&root);
        std::fs::write(&pack, &pack_bytes).unwrap();
        let digest: [u8; 16] = Md5::digest(&pack_bytes).into();
        let mut index_bytes = Vec::new();
        index_bytes.extend_from_slice(&0x6934_0304u32.to_le_bytes());
        index_bytes.extend_from_slice(&200u32.to_le_bytes());
        index_bytes.extend_from_slice(&source.header.export_version.to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&digest);
        std::fs::write(&index, index_bytes).unwrap();

        let peek = source_version(&index).unwrap();
        assert_eq!(peek.version, 200);
        assert!(!peek.supported);
        let info = inspect(&index).unwrap();
        assert!(!info.supported);
        assert_eq!(info.closest_version, 184);
        let report = analyze(&index, 165).unwrap();
        assert!(report.exact_round_trip);
        assert_eq!(report.exact_roots, 1);
        assert!(report.first_issue.is_none());
        let comparison = compare_ids(&index, source_path).unwrap();
        assert_eq!(comparison.matched_ids, 1);
        assert_eq!(comparison.same_size, 1);
        assert_eq!(comparison.target_only, 0);
        assert_eq!(comparison.reference_only, source.header.root_count as usize - 1);
        assert!(comparison.size_patterns.iter().any(|pattern| pattern.delta == 0 && pattern.count == 1));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn extra_bytes_are_reported_at_the_first_trailing_offset() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        let source = TaskContainer::open(source_path).unwrap();
        let mut bytes = source.root(0, 0).unwrap();
        let expected = bytes.len();
        bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let schema = schema_for_version(165).unwrap();
        let (_, used) = decode_prefix_diagnostic(&schema, &bytes, 165).unwrap();
        assert_eq!(used, expected);
    }

    #[test]
    fn fixed_field_scoring_finds_an_inserted_raw32_boundary() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        let source = TaskContainer::open(source_path).unwrap();
        let mut root = source.root(0, 0).unwrap();
        root.splice(64..64, [0x12, 0x34, 0x56, 0x78]);
        let (folder, index) = single_root_set(&root, 200, source.header.export_version, "field-score");
        let report = score_fixed_fields(&index, source_path).unwrap();
        assert_eq!(report.matched_ids, 1);
        assert_eq!(report.sampled_roots, 1);
        let candidate = report.candidates.iter().find(|candidate| candidate.width == 4 && candidate.min_offset == 64)
            .expect("the four inserted bytes should realign at the name boundary");
        assert_eq!(candidate.after_field, "name");
        assert_eq!(candidate.example_values, vec!["12 34 56 78"]);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn fixed_field_scoring_normalizes_an_accepted_insertion_before_finding_the_next() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        let source = TaskContainer::open(source_path).unwrap();
        let mut root = source.root(0, 0).unwrap();
        root.splice(64..64, [0x12, 0x34, 0x56, 0x78]);
        root.splice(68..68, [0xAB, 0xCD]);
        let (folder, index) = single_root_set(&root, 200, source.header.export_version, "field-score-next");
        let mut accepted = super::super::schema::FieldDef::new("accepted_v200_1", FieldType::Raw { len: 4 });
        accepted.when.push(Condition::Field { field: "id".into(), predicate: super::super::schema::Predicate::NonZero });
        let operations = vec![PatchOperation::InsertAfter {
            structure: "TASK_FIXED_V165".into(),
            after: Some("name".into()),
            field: accepted,
        }];
        let report = score_fixed_fields_with_operations(&index, source_path, &operations).unwrap();
        let candidate = report.candidates.iter().find(|candidate| candidate.width == 2 && candidate.min_offset == 64 && candidate.after_field == "accepted_v200_1")
            .unwrap_or_else(|| panic!("the second field should anchor after the accepted field: {:#?}", report.candidates));
        assert_eq!(candidate.example_values, vec!["AB CD"]);
        std::fs::remove_dir_all(folder).unwrap();
    }

}
