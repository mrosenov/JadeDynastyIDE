//! Another aipolicy.data (or a `jdide-aipolicy` JSON export) next to the open one: comparison by policy and
//! trigger ID, copying policies or triggers across, JSON export, and taking texts over (translation).

use std::collections::BTreeMap;

use super::*;

/// The other file: read-only.
pub struct Compared {
    pub path: String,
    pub file: PolicyFile,
}

impl Compared {
    pub fn open(path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|error| format!("Could not read {path}: {error}"))?;
        let file = if path.to_lowercase().ends_with(".json") {
            let text = String::from_utf8(data).map_err(|_| format!("{path} is not UTF-8 text"))?;
            from_json(&text).map_err(|error| format!("{path}: {error}"))?
        } else {
            parse(&data).map_err(|error| format!("{path}: {error}"))?
        };
        Ok(Compared { path: path.to_string(), file })
    }
}

/// Two triggers do the same (the trigger version is ignored: versions 9 and later store the same bytes).
pub fn same_trigger(a: &Trigger, b: &Trigger) -> bool {
    a.id == b.id && a.active == b.active && a.run == b.run && (a.run_condition != 0) == (b.run_condition != 0) && trigger_name(a) == trigger_name(b) && a.condition == b.condition && a.operations == b.operations
}

fn same_policy(a: &Policy, b: &Policy) -> bool {
    a.triggers.len() == b.triggers.len() && a.triggers.iter().zip(&b.triggers).all(|(a, b)| same_trigger(a, b))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerPair {
    pub id: u32,
    pub here: Option<usize>,
    pub there: Option<usize>,
    /// `same`, `changed`, `here` (only in the open file), `there` (only in the other).
    pub status: &'static str,
}

/// Triggers paired by ID (repeated IDs by their order), in the open file's order, then the other's.
pub fn pair_triggers(here: Option<&Policy>, there: Option<&Policy>) -> Vec<TriggerPair> {
    let empty = Vec::new();
    let a = here.map_or(&empty, |policy| &policy.triggers);
    let b = there.map_or(&empty, |policy| &policy.triggers);
    let mut queues: HashMap<u32, std::collections::VecDeque<usize>> = HashMap::new();
    for (index, trigger) in b.iter().enumerate() {
        queues.entry(trigger.id).or_default().push_back(index);
    }
    let mut out = Vec::new();
    for (index, trigger) in a.iter().enumerate() {
        let other = queues.get_mut(&trigger.id).and_then(|queue| queue.pop_front());
        let status = match other {
            Some(other) if same_trigger(trigger, &b[other]) => "same",
            Some(_) => "changed",
            None => "here",
        };
        out.push(TriggerPair { id: trigger.id, here: Some(index), there: other, status });
    }
    let paired: HashSet<usize> = out.iter().filter_map(|pair| pair.there).collect();
    for (index, trigger) in b.iter().enumerate() {
        if !paired.contains(&index) {
            out.push(TriggerPair { id: trigger.id, here: None, there: Some(index), status: "there" });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareRow {
    pub id: u32,
    /// Positions of the policy the server uses (the first of its ID) in each file.
    pub here: Option<usize>,
    pub there: Option<usize>,
    pub status: &'static str,
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
    /// The same triggers in another order (the server tests them in order).
    pub reordered: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareSummary {
    pub path: String,
    /// The other file is for the newer server.
    pub newer: bool,
    pub rows: Vec<CompareRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDiff {
    pub here: Option<PolicyView>,
    pub there: Option<PolicyView>,
    pub triggers: Vec<TriggerPair>,
}

pub fn compare(here: &PolicyFile, compared: &Compared) -> CompareSummary {
    let first_here = first_of_ids(&here.policies);
    let first_there = first_of_ids(&compared.file.policies);
    let row = |id: u32| {
        let a = first_here.get(&id).copied();
        let b = first_there.get(&id).copied();
        let pairs = pair_triggers(a.map(|index| &here.policies[index]), b.map(|index| &compared.file.policies[index]));
        let count = |status: &str| pairs.iter().filter(|pair| pair.status == status).count();
        let status = match (a, b) {
            (Some(a), Some(b)) if same_policy(&here.policies[a], &compared.file.policies[b]) => "same",
            (Some(_), Some(_)) => "changed",
            (Some(_), None) => "here",
            _ => "there",
        };
        let (changed, added, removed) = (count("changed"), count("there"), count("here"));
        CompareRow { id, here: a, there: b, status, changed, added, removed, reordered: status == "changed" && changed + added + removed == 0 }
    };
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for policy in here.policies.iter().chain(&compared.file.policies) {
        if seen.insert(policy.id) {
            rows.push(row(policy.id));
        }
    }
    CompareSummary { path: compared.path.clone(), newer: is_newer(&compared.file), rows }
}

pub fn policy_diff(here: &PolicyFile, compared: &Compared, id: u32) -> Result<PolicyDiff, String> {
    let a = first_of_ids(&here.policies).get(&id).copied();
    let b = first_of_ids(&compared.file.policies).get(&id).copied();
    if a.is_none() && b.is_none() {
        return Err(format!("Neither file has policy {id}"));
    }
    Ok(PolicyDiff {
        here: a.map(|index| policy_view(&here.policies, index)).transpose()?,
        there: b.map(|index| policy_view(&compared.file.policies, index)).transpose()?,
        triggers: pair_triggers(a.map(|index| &here.policies[index]), b.map(|index| &compared.file.policies[index])),
    })
}

/// Types the 2013 server does not read, in a trigger (none: it can go into a file for that server).
fn newer_only(trigger: &Trigger) -> Option<String> {
    fn walk(node: &Condition) -> Option<i32> {
        if !condition_known(node.kind, false) {
            return Some(node.kind);
        }
        node.left.as_deref().and_then(walk).or_else(|| node.right.as_deref().and_then(walk))
    }
    if let Some(kind) = walk(&trigger.condition) {
        return Some(format!("\"{}\"", condition_name(kind)));
    }
    for operation in &trigger.operations {
        if !operation_known(operation.kind, false) {
            return Some(format!("\"{}\"", operation_name(operation.kind)));
        }
        if !target_known(operation.target, false) {
            return Some(format!("the target \"{}\"", target_name(operation.target)));
        }
    }
    None
}

/// A trigger as the destination's server reads it: refused when it uses newer types; version 12 becomes 11 for
/// the 2013 server (the same bytes; a version-12 trigger would make the file look like a newer server's).
fn adapt(trigger: &Trigger, newer: bool) -> Result<Trigger, String> {
    let mut out = trigger.clone();
    if !newer {
        if let Some(what) = newer_only(trigger) {
            return Err(format!("trigger {} uses {what}, which the 2013 server does not read", trigger.id));
        }
        out.version = out.version.min(11);
    }
    Ok(out)
}

/// What to copy: a whole policy (`triggers` none) or some of its triggers by ID.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyItem {
    pub id: u32,
    #[serde(default)]
    pub triggers: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyReport {
    pub policies_added: usize,
    pub policies_replaced: usize,
    pub triggers_added: usize,
    pub triggers_replaced: usize,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    pub result: EditResult,
    pub report: CopyReport,
}

impl Document {
    /// Copies policies (replacing the one of the same ID, or inserted in ID order) or triggers (replacing the
    /// one of the same ID, or appended) from the other file, as one undo step.
    pub fn copy_compared(&mut self, compared: &Compared, items: &[CopyItem], shown: Option<usize>) -> Result<CopyResult, String> {
        let newer = is_newer(&self.file);
        let source_first = first_of_ids(&compared.file.policies);
        let mut working = self.file.policies.clone();
        let mut changes = Vec::new();
        let mut report = CopyReport::default();
        for item in items {
            let Some(&from) = source_first.get(&item.id) else {
                report.skipped.push(format!("Policy {}: not in the other file", item.id));
                continue;
            };
            let source = &compared.file.policies[from];
            let here = working.iter().position(|policy| policy.id == item.id);
            match (&item.triggers, here) {
                (Some(ids), Some(at)) => {
                    let before = working[at].clone();
                    let mut after = before.clone();
                    for id in ids {
                        let Some(trigger) = source.triggers.iter().find(|trigger| trigger.id == *id) else {
                            report.skipped.push(format!("Policy {}: no trigger {id} in the other file", item.id));
                            continue;
                        };
                        match adapt(trigger, newer) {
                            Ok(trigger) => match after.triggers.iter().position(|entry| entry.id == *id) {
                                Some(position) => {
                                    after.triggers[position] = trigger;
                                    report.triggers_replaced += 1;
                                }
                                None => {
                                    after.triggers.push(trigger);
                                    report.triggers_added += 1;
                                }
                            },
                            Err(error) => report.skipped.push(format!("Policy {}: {error}", item.id)),
                        }
                    }
                    if after != before {
                        working[at] = after.clone();
                        changes.push(PolicyChange { index: at, before: Some(before), after: Some(after) });
                    }
                }
                (_, here) => {
                    let triggers: Result<Vec<Trigger>, String> = source.triggers.iter().map(|trigger| adapt(trigger, newer)).collect();
                    let triggers = match triggers {
                        Ok(triggers) => triggers,
                        Err(error) => {
                            report.skipped.push(format!("Policy {}: {error}", item.id));
                            continue;
                        }
                    };
                    let policy = Policy { version: source.version, id: source.id, triggers };
                    match here {
                        Some(at) => {
                            if working[at] != policy {
                                changes.push(PolicyChange { index: at, before: Some(working[at].clone()), after: Some(policy.clone()) });
                                working[at] = policy;
                                report.policies_replaced += 1;
                            }
                        }
                        None => {
                            let at = working.iter().position(|entry| entry.id > policy.id).unwrap_or(working.len());
                            changes.push(PolicyChange { index: at, before: None, after: Some(policy.clone()) });
                            working.insert(at, policy);
                            report.policies_added += 1;
                        }
                    }
                }
            }
        }
        if changes.is_empty() {
            return Ok(CopyResult { result: self.result(shown, false)?, report });
        }
        let parts: Vec<String> = [
            (report.policies_added, "policy added", "policies added"),
            (report.policies_replaced, "policy replaced", "policies replaced"),
            (report.triggers_added, "trigger added", "triggers added"),
            (report.triggers_replaced, "trigger replaced", "triggers replaced"),
        ]
        .iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, one, many)| format!("{count} {}", if *count == 1 { one } else { many }))
        .collect();
        self.record(format!("Copy from {}: {}", file_name(&compared.path), parts.join(", ")), changes);
        // The shown policy may have moved when policies were inserted before it.
        let shown = shown.and_then(|index| self.file.policies.get(index).map(|_| index));
        Ok(CopyResult { result: self.result(shown, true)?, report })
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

// ── JSON ──

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    if text.len() % 2 != 0 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("\"{}\" is not hex bytes", text.chars().take(24).collect::<String>()));
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(|error| error.to_string())).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionJson {
    #[serde(rename = "type")]
    pub kind: i32,
    /// For reading only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, ParamValue>,
    /// The stored parameter (hex); values are written over it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<Box<ConditionJson>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<Box<ConditionJson>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJson {
    #[serde(rename = "type")]
    pub kind: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, ParamValue>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw: String,
    pub target: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_mask: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerJson {
    pub id: u32,
    pub version: u32,
    pub name: String,
    /// The stored 128-byte name slot (hex), kept while it still says `name`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name_raw: String,
    pub active: bool,
    pub run: bool,
    pub battle_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_condition: Option<u8>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub expression: String,
    pub condition: ConditionJson,
    pub operations: Vec<OperationJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyJson {
    pub id: u32,
    pub triggers: Vec<TriggerJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiJson {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub newer: bool,
    pub policies: Vec<PolicyJson>,
}

pub const JSON_FORMAT: &str = "jdide-aipolicy";

fn values_of(params: Vec<Param>) -> BTreeMap<String, ParamValue> {
    params.into_iter().map(|param| (param.key.to_string(), param.value)).collect()
}

fn condition_json(node: &Condition) -> ConditionJson {
    ConditionJson {
        kind: node.kind,
        name: condition_name(node.kind).to_string(),
        values: values_of(condition_params(node)),
        raw: hex(&node.param),
        left: node.left.as_deref().map(|child| Box::new(condition_json(child))),
        right: node.right.as_deref().map(|child| Box::new(condition_json(child))),
    }
}

fn trigger_json(trigger: &Trigger) -> TriggerJson {
    TriggerJson {
        id: trigger.id,
        version: trigger.version,
        name: trigger_name(trigger),
        name_raw: hex(&trigger.name),
        active: trigger.active,
        run: trigger.run,
        battle_only: trigger.run_condition != 0,
        run_condition: (trigger.run_condition > 1).then_some(trigger.run_condition),
        expression: expression(&trigger.condition),
        condition: condition_json(&trigger.condition),
        operations: trigger
            .operations
            .iter()
            .map(|operation| OperationJson {
                kind: operation.kind,
                name: operation_name(operation.kind).to_string(),
                values: values_of(operation_params(operation, trigger.version)),
                // Texts are their value.
                raw: if matches!(operation.kind, O_TALK | O_WHISPER | O_TALK_PORTRAIT) { String::new() } else { hex(&operation.param) },
                target: operation.target,
                target_mask: operation.target_param,
            })
            .collect(),
    }
}

/// The policies at `indexes` (all when none) as `jdide-aipolicy` JSON.
pub fn to_json(file: &PolicyFile, indexes: Option<&[usize]>, source: &str) -> Result<String, String> {
    let policies: Vec<&Policy> = match indexes {
        Some(indexes) => indexes.iter().map(|&index| file.policies.get(index).ok_or_else(|| format!("No policy {}", index + 1))).collect::<Result<_, _>>()?,
        None => file.policies.iter().collect(),
    };
    let json = AiJson {
        format: JSON_FORMAT.into(),
        format_version: 1,
        source: source.into(),
        newer: is_newer(file),
        policies: policies.into_iter().map(|policy| PolicyJson { id: policy.id, triggers: policy.triggers.iter().map(trigger_json).collect() }).collect(),
    };
    serde_json::to_string_pretty(&json).map_err(|error| error.to_string())
}

fn condition_edit(json: &ConditionJson) -> Result<ConditionEdit, String> {
    Ok(ConditionEdit {
        kind: json.kind,
        values: json.values.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
        raw: unhex(&json.raw)?,
        left: json.left.as_deref().map(condition_edit).transpose()?.map(Box::new),
        right: json.right.as_deref().map(condition_edit).transpose()?.map(Box::new),
    })
}

fn trigger_from_json(json: &TriggerJson) -> Result<Trigger, String> {
    let what = format!("trigger {}", json.id);
    let stored = unhex(&json.name_raw).map_err(|error| format!("{what}: name: {error}"))?;
    let name_matches = stored.len() == 128 && {
        let end = stored.iter().position(|&byte| byte == 0).unwrap_or(stored.len());
        GBK.decode_without_bom_handling(&stored[..end]).0 == json.name
    };
    // build_trigger keeps the old name slot while the name is unchanged.
    let old = Trigger {
        version: json.version,
        id: json.id,
        active: json.active,
        run: json.run,
        run_condition: json.run_condition.filter(|&value| (value != 0) == json.battle_only).unwrap_or(json.battle_only as u8),
        name: if name_matches { stored } else { vec![0; 128] },
        condition: Condition { kind: C_RANDOM, param: vec![0; 4], left: None, right: None },
        operations: Vec::new(),
    };
    let edit = TriggerEdit {
        name: json.name.clone(),
        active: json.active,
        run: json.run,
        battle_only: json.battle_only,
        condition: condition_edit(&json.condition).map_err(|error| format!("{what}: {error}"))?,
        operations: json
            .operations
            .iter()
            .map(|operation| {
                Ok(OperationEdit {
                    kind: operation.kind,
                    values: operation.values.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
                    raw: unhex(&operation.raw)?,
                    target: operation.target,
                    target_mask: operation.target_mask,
                })
            })
            .collect::<Result<_, String>>()
            .map_err(|error| format!("{what}: {error}"))?,
    };
    let mut trigger = build_trigger(&old, &edit, true).map_err(|error| format!("{what}: {error}"))?;
    if name_matches {
        trigger.name = old.name;
    }
    Ok(trigger)
}

pub fn from_json(text: &str) -> Result<PolicyFile, String> {
    let json: AiJson = serde_json::from_str(text).map_err(|error| format!("not a JD IDE aipolicy JSON export: {error}"))?;
    if json.format != JSON_FORMAT {
        return Err(format!("the format is \"{}\", not \"{JSON_FORMAT}\"", json.format));
    }
    if json.format_version != 1 {
        return Err(format!("format version {} is newer than this JD IDE reads (1)", json.format_version));
    }
    let policies = json
        .policies
        .iter()
        .map(|policy| Ok(Policy { version: 0, id: policy.id, triggers: policy.triggers.iter().map(trigger_from_json).collect::<Result<_, String>>().map_err(|error| format!("policy {}: {error}", policy.id))? }))
        .collect::<Result<_, String>>()?;
    Ok(PolicyFile { version: 0, policies })
}

// ── Taking texts over (translation) ──

/// A text the other file has differently: a trigger name, a talk text or a summoned monster's name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextUpdate {
    pub policy: usize,
    pub policy_id: u32,
    pub trigger: usize,
    pub trigger_id: u32,
    /// The operation (none: the trigger's name).
    pub operation: Option<usize>,
    /// `name`, `talk` or `summon`.
    pub group: String,
    pub current: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationPreview {
    pub updates: Vec<TextUpdate>,
    /// Paired triggers whose operations differ in type (their texts are not taken).
    pub skipped_triggers: usize,
    /// Texts the open file cannot store (GBK names, 16-character summon names).
    pub too_long: usize,
}

fn summon_name(operation: &Operation) -> String {
    utf16(operation.param.get(16..48).unwrap_or(&[]))
}

fn texts_of(operation: &Operation) -> Option<(&'static str, String)> {
    match operation.kind {
        O_TALK | O_WHISPER | O_TALK_PORTRAIT => Some(("talk", utf16(&operation.param))),
        15 => Some(("summon", summon_name(operation))),
        _ => None,
    }
}

fn gbk_fits(text: &str) -> bool {
    let (encoded, _, unmappable) = GBK.encode(text);
    !unmappable && encoded.len() <= 127
}

impl Document {
    /// Texts of paired policies and triggers (by ID) that the other file has differently. Operations pair by
    /// position when both triggers have the same operation types; empty texts are never taken.
    pub fn translation(&self, source: &PolicyFile) -> TranslationPreview {
        let mut preview = TranslationPreview::default();
        let first_here = first_of_ids(&self.file.policies);
        let first_there = first_of_ids(&source.policies);
        for (index, policy) in self.file.policies.iter().enumerate() {
            if first_here.get(&policy.id) != Some(&index) {
                continue;
            }
            let Some(&other) = first_there.get(&policy.id) else { continue };
            let other = &source.policies[other];
            for pair in pair_triggers(Some(policy), Some(other)) {
                let (Some(at), Some(from)) = (pair.here, pair.there) else { continue };
                let (trigger, theirs) = (&policy.triggers[at], &other.triggers[from]);
                let push = |preview: &mut TranslationPreview, operation: Option<usize>, group: &str, current: String, text: String, fits: bool| {
                    if text.trim().is_empty() || text == current {
                        return;
                    }
                    if !fits {
                        preview.too_long += 1;
                        return;
                    }
                    preview.updates.push(TextUpdate { policy: index, policy_id: policy.id, trigger: at, trigger_id: trigger.id, operation, group: group.into(), current, text });
                };
                let name = trigger_name(theirs);
                push(&mut preview, None, "name", trigger_name(trigger), name.clone(), gbk_fits(&name));
                let kinds = |trigger: &Trigger| trigger.operations.iter().map(|operation| operation.kind).collect::<Vec<_>>();
                if kinds(trigger) != kinds(theirs) {
                    if trigger.operations.iter().any(|operation| texts_of(operation).is_some()) {
                        preview.skipped_triggers += 1;
                    }
                    continue;
                }
                for (number, (mine, other)) in trigger.operations.iter().zip(&theirs.operations).enumerate() {
                    let (Some((group, current)), Some((_, text))) = (texts_of(mine), texts_of(other)) else { continue };
                    let fits = group != "summon" || text.encode_utf16().count() <= 16;
                    let fits = fits && text.encode_utf16().count() <= 2000;
                    push(&mut preview, Some(number), group, current, text, fits);
                }
            }
        }
        preview
    }

    /// Writes the chosen texts (one undo step); refuses texts that changed since the preview.
    pub fn apply_texts(&mut self, updates: &[TextUpdate], shown: Option<usize>) -> Result<EditResult, String> {
        let mut by_policy: BTreeMap<usize, Policy> = BTreeMap::new();
        for update in updates {
            if !by_policy.contains_key(&update.policy) {
                let policy = self.file.policies.get(update.policy).filter(|policy| policy.id == update.policy_id).ok_or_else(|| format!("Policy {} moved since the preview; preview again", update.policy_id))?;
                by_policy.insert(update.policy, policy.clone());
            }
            let policy = by_policy.get_mut(&update.policy).unwrap();
            let trigger = policy.triggers.get_mut(update.trigger).filter(|trigger| trigger.id == update.trigger_id).ok_or_else(|| format!("Trigger {} of policy {} moved since the preview; preview again", update.trigger_id, update.policy_id))?;
            let stale = || format!("A text of trigger {} in policy {} changed since the preview; preview again", update.trigger_id, update.policy_id);
            match update.operation {
                None => {
                    if trigger_name(trigger) != update.current {
                        return Err(stale());
                    }
                    let (encoded, _, unmappable) = GBK.encode(&update.text);
                    if unmappable || encoded.len() > 127 {
                        return Err(format!("The name of trigger {} does not fit (GBK, 127 bytes)", trigger.id));
                    }
                    let mut slot = vec![0u8; 128];
                    slot[..encoded.len()].copy_from_slice(&encoded);
                    trigger.name = slot;
                }
                Some(number) => {
                    let operation = trigger.operations.get_mut(number).ok_or_else(stale)?;
                    if texts_of(operation).map(|(_, text)| text) != Some(update.current.clone()) {
                        return Err(stale());
                    }
                    if operation.kind == 15 {
                        let bytes = utf16_bytes(&update.text, false);
                        if bytes.len() > 32 {
                            return Err(format!("A summon name in trigger {} has more than 16 characters", trigger.id));
                        }
                        let slot = &mut operation.param[16..48];
                        slot.fill(0);
                        slot[..bytes.len()].copy_from_slice(&bytes);
                    } else {
                        operation.param = utf16_bytes(&update.text, true);
                    }
                }
            }
        }
        let changes: Vec<PolicyChange> = by_policy
            .into_iter()
            .filter(|(index, after)| &self.file.policies[*index] != after)
            .map(|(index, after)| PolicyChange { index, before: Some(self.file.policies[index].clone()), after: Some(after) })
            .collect();
        if !changes.is_empty() {
            let count = updates.len();
            self.record(format!("Take over {count} text{}", if count == 1 { "" } else { "s" }), changes);
        }
        self.result(shown, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<String> {
        let base = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/".into());
        let base = base.trim_end_matches(['/', '\\']);
        [
            "Games/XtremeJade/element/data/aipolicy.data",
            "Games/ForsakenJD/element/data/aipolicy.data",
            "Game Dev/JD/1559/gamed/config/aipolicy.data",
            "Game Dev/JD/zxserver/zgame/gs/config/aipolicy.data",
            "Game Dev/JD/zx_18_compiled/gamed/config/aipolicy.data",
            "Game Dev/JD/zx_18_compiled/gamed/config/aipolicy.data-cn",
        ]
        .iter()
        .map(|path| format!("{base}/{path}"))
        .filter(|path| std::path::Path::new(path).is_file())
        .collect()
    }

    #[test]
    fn json_round_trips_every_sample_byte_for_byte() {
        for path in samples() {
            let data = std::fs::read(&path).unwrap();
            let file = parse(&data).unwrap();
            let json = to_json(&file, None, &path).unwrap();
            let back = from_json(&json).unwrap();
            assert!(encode(&back) == data, "{path}");
        }
    }

    #[test]
    fn json_values_are_editable() {
        let Some(path) = samples().into_iter().next() else { return };
        let file = parse(&std::fs::read(&path).unwrap()).unwrap();
        let index = file.policies.iter().position(|policy| policy.triggers.iter().any(|trigger| trigger.operations.iter().any(|operation| operation.kind == O_TALK))).unwrap();
        let mut json: AiJson = serde_json::from_str(&to_json(&file, Some(&[index]), &path).unwrap()).unwrap();
        let trigger = json.policies[0].triggers.iter_mut().find(|trigger| trigger.operations.iter().any(|operation| operation.kind == O_TALK)).unwrap();
        trigger.name = "Edited".into();
        let talk = trigger.operations.iter_mut().find(|operation| operation.kind == O_TALK).unwrap();
        talk.values.insert("text".into(), ParamValue::Text("$BHello".into()));
        let id = trigger.id;
        let back = from_json(&serde_json::to_string(&json).unwrap()).unwrap();
        let trigger = back.policies[0].triggers.iter().find(|trigger| trigger.id == id).unwrap();
        assert_eq!(trigger_name(trigger), "Edited");
        assert!(trigger.operations.iter().any(|operation| operation.kind == O_TALK && utf16(&operation.param) == "$BHello"));
    }

    #[test]
    fn compare_copy_and_translate_between_samples() {
        let samples = samples();
        // zxserver (2013 server) against 1559 (newer server).
        let (Some(here), Some(there)) = (samples.iter().find(|path| path.contains("zxserver")), samples.iter().find(|path| path.contains("1559"))) else { return };
        let mut document = Document::open(here).unwrap();
        let compared = Compared::open(there).unwrap();
        let summary = compare(&document.file, &compared);
        let count = |status: &str| summary.rows.iter().filter(|row| row.status == status).count();
        println!("COMPARE same {} changed {} here {} there {}", count("same"), count("changed"), count("here"), count("there"));
        assert!(!is_newer(&document.file) && summary.newer);
        // A file compared with itself is all the same.
        let itself = Compared::open(here).unwrap();
        assert!(compare(&document.file, &itself).rows.iter().all(|row| row.status == "same"));

        // Copy everything: what the 2013 server can read lands; the file stays a 2013 file.
        let items: Vec<CopyItem> = summary.rows.iter().filter(|row| row.status != "same" && row.there.is_some()).map(|row| CopyItem { id: row.id, triggers: None }).collect();
        let original = document.file.clone();
        let copied = document.copy_compared(&compared, &items, None).unwrap();
        println!("COPY {:?}", copied.report.skipped.len());
        assert!(!is_newer(&document.file));
        let after = compare(&document.file, &compared);
        let skipped: HashSet<u32> = copied.report.skipped.iter().filter_map(|line| line.strip_prefix("Policy ")?.split(':').next()?.parse().ok()).collect();
        for row in after.rows.iter().filter(|row| row.there.is_some() && !skipped.contains(&row.id)) {
            // Only the trigger versions may differ (11 here, 12 there).
            let a = &document.file.policies[row.here.unwrap()];
            let b = &compared.file.policies[row.there.unwrap()];
            assert!(same_policy(a, b), "policy {}", row.id);
        }
        document.undo().unwrap();
        assert_eq!(document.file, original);

        // Texts: compared with itself nothing differs; with the other file only paired triggers' texts.
        assert!(document.translation(&itself.file).updates.is_empty());
        let preview = document.translation(&compared.file);
        println!("TRANSLATE {} updates, {} skipped, {} too long", preview.updates.len(), preview.skipped_triggers, preview.too_long);
        if !preview.updates.is_empty() {
            document.apply_texts(&preview.updates, None).unwrap();
            assert!(document.translation(&compared.file).updates.is_empty());
            document.undo().unwrap();
            assert_eq!(document.file, original);
        }
    }
}

