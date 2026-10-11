//! `aipolicy.data`: the monster AI the server runs (gs.conf `PolicyData`; the client never reads it). A monster
//! uses the policy whose ID is its `MONSTER_ESSENCE.common_strategy` (template_loader.cpp `trigger_policy =
//! common_strategy`; a missing policy is printed and dropped). Layout fields with the `aipolicy` role link here.
//!
//! Format (`CPolicyDataManager`/`CPolicyData`/`CTriggerData::Load` in the official editor's ZElementData
//! Policy.cpp, identical to the server's gs/ai/policy.cpp): u32 version 0, i32 policy count; per policy u32
//! version 0, u32 ID, i32 trigger count; per trigger u32 version (11 in the 2013 source, 12 in the newer server),
//! u32 ID, bool active, bool run (a sub-trigger, only run by another trigger), u8 run condition (1 = battle
//! only), char name[128] (GBK), the condition tree, i32 operation count, operations. A tree node is i32 type,
//! i32 parameter size, the parameter, then flags: 3 = leaf, else 1 + left child and/or 2 + right child, then 4.
//! An operation is i32 type, its parameter (a size fixed per type and version, `policytype.h`, not packed:
//! summon is 56 bytes; talk, whisper and portrait talk are u32 size + UTF-16), i32 target type (+ u32 class mask
//! for the class target). Version 0 triggers read parameters for fewer types; before version 9 "change path"
//! has no type. All six samples read to their last byte and write back byte for byte.
//!
//! Runtime (gs aitrigger.h, ai/policy_loader.cpp): run triggers are not added on their own; a trigger's root
//! condition picks its list (and/or take their left child's, not its child's): heartbeat, timer, kill target,
//! combat start, death, path end, birth, skill hit, leave combat. HP below, aggro count, distance and skill hit
//! disable their trigger after firing; random select-one runs one random operation. The server keeps the first
//! policy of an ID (`hashtab::put`).

use std::collections::{HashMap, HashSet};

use encoding_rs::GBK;
use serde::{Deserialize, Serialize};

pub mod compare;

// ── Condition, operation and target types (CTriggerData enums) ──

pub const C_TIME_COME: i32 = 0;
pub const C_HP_LESS: i32 = 1;
pub const C_START_ATTACK: i32 = 2;
pub const C_RANDOM: i32 = 3;
pub const C_KILL_PLAYER: i32 = 4;
pub const C_NOT: i32 = 5;
pub const C_OR: i32 = 6;
pub const C_AND: i32 = 7;
pub const C_DIED: i32 = 8;
pub const C_PATH_END: i32 = 9;
pub const C_ENMITY_REACH: i32 = 10;
pub const C_DISTANCE_REACH: i32 = 11;
pub const C_PLUS: i32 = 12;
pub const C_DIVIDE: i32 = 15;
pub const C_GREAT: i32 = 16;
pub const C_EQU: i32 = 18;
pub const C_VAR: i32 = 19;
pub const C_CONSTANT: i32 = 20;
pub const C_RANK_LEVEL: i32 = 21;
pub const C_BORN: i32 = 22;
pub const C_ATTACK_BY_SKILL: i32 = 23;
pub const C_RANDOM_SELECTONE: i32 = 24;
pub const C_LEAVE_COMBAT: i32 = 25;

pub const O_TALK: i32 = 2;
pub const O_RUN_TRIGGER: i32 = 4;
pub const O_STOP_TRIGGER: i32 = 5;
pub const O_ACTIVE_TRIGGER: i32 = 6;
pub const O_CREATE_TIMER: i32 = 7;
pub const O_KILL_TIMER: i32 = 8;
pub const O_ACTIVE_CONTROLLER: i32 = 14;
pub const O_CHANGE_PATH: i32 = 17;
pub const O_WHISPER: i32 = 31;
pub const O_TALK_PORTRAIT: i32 = 32;

pub const T_OCCUPATION_LIST: i32 = 6;

const CONDITION_LEFT: i32 = 1;
const CONDITION_RIGHT: i32 = 2;
const CONDITION_LEAF: i32 = 3;
const CONDITION_END: i32 = 4;

/// The parameter size of an operation in a trigger of `version` (none: a text, size-prefixed).
fn operation_size(kind: i32, version: u32) -> Result<Option<usize>, String> {
    if matches!(kind, O_TALK | O_WHISPER | O_TALK_PORTRAIT) {
        return Ok(None);
    }
    if version == 0 {
        // Version 0 read parameters only for these (and 4 bytes for the controller).
        return Ok(Some(match kind {
            0 | 4 | 5 | 6 | 8 | O_ACTIVE_CONTROLLER => 4,
            1 => 8,
            7 | 20 => 12,
            15 => 56,
            _ => 0,
        }));
    }
    Ok(Some(match kind {
        0 | 4 | 5 | 6 | 8 | 16 | 19 | 27 | 28 | 29 => 4,
        1 | 22 | 23 | 24 => 8,
        O_ACTIVE_CONTROLLER => 8,
        O_CHANGE_PATH => {
            if version < 9 {
                4
            } else {
                8
            }
        }
        7 | 20 | 26 | 30 => 12,
        15 => 56,
        25 => 20,
        3 | 9 | 10 | 11 | 12 | 13 | 18 | 21 => 0,
        other => return Err(format!("unknown operation type {other}")),
    }))
}

// ── The file ──

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub kind: i32,
    pub param: Vec<u8>,
    pub left: Option<Box<Condition>>,
    pub right: Option<Box<Condition>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    pub kind: i32,
    /// The parameter as stored (texts: the UTF-16 bytes without their size).
    pub param: Vec<u8>,
    pub target: i32,
    pub target_param: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    pub version: u32,
    pub id: u32,
    pub active: bool,
    pub run: bool,
    pub run_condition: u8,
    /// The name slot as stored (GBK; usually with bytes after the terminator).
    pub name: Vec<u8>,
    pub condition: Condition,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    pub version: u32,
    pub id: u32,
    pub triggers: Vec<Trigger>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PolicyFile {
    pub version: u32,
    pub policies: Vec<Policy>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(size).filter(|&end| end <= self.data.len()).ok_or_else(|| format!("the file ends at {} inside a record at {}", self.data.len(), self.at))?;
        let slice = &self.data[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn count(&mut self, what: &str) -> Result<usize, String> {
        let at = self.at;
        let count = self.i32()?;
        usize::try_from(count).ok().filter(|&count| count <= 1_000_000).ok_or_else(|| format!("invalid {what} count {count} at {at}"))
    }

    fn condition(&mut self, depth: usize) -> Result<Condition, String> {
        if depth > 64 {
            return Err(format!("a condition tree deeper than 64 at {}", self.at));
        }
        let kind = self.i32()?;
        let at = self.at;
        let size = self.i32()?;
        if !(0..=64).contains(&size) {
            return Err(format!("condition parameter size {size} at {at}"));
        }
        let param = self.take(size as usize)?.to_vec();
        let mut node = Condition { kind, param, left: None, right: None };
        loop {
            let at = self.at;
            match self.i32()? {
                CONDITION_LEAF | CONDITION_END => break,
                CONDITION_LEFT => node.left = Some(Box::new(self.condition(depth + 1)?)),
                CONDITION_RIGHT => node.right = Some(Box::new(self.condition(depth + 1)?)),
                flag => return Err(format!("condition tree flag {flag} at {at}")),
            }
        }
        Ok(node)
    }

    fn trigger(&mut self) -> Result<Trigger, String> {
        let version = self.u32()?;
        let id = self.u32()?;
        let active = self.u8()? != 0;
        let run = self.u8()? != 0;
        let run_condition = self.u8()?;
        let name = self.take(128)?.to_vec();
        let condition = self.condition(0)?;
        let count = self.count("operation")?;
        let mut operations = Vec::with_capacity(count);
        for _ in 0..count {
            let at = self.at;
            let kind = self.i32()?;
            let param = match operation_size(kind, version).map_err(|error| format!("{error} at {at} (trigger {id})"))? {
                Some(size) => self.take(size)?.to_vec(),
                None => {
                    let size = self.u32()? as usize;
                    if size > 1 << 20 {
                        return Err(format!("text of {size} bytes at {at}"));
                    }
                    self.take(size)?.to_vec()
                }
            };
            let target = self.i32()?;
            let target_param = if target == T_OCCUPATION_LIST { Some(self.u32()?) } else { None };
            operations.push(Operation { kind, param, target, target_param });
        }
        Ok(Trigger { version, id, active, run, run_condition, name, condition, operations })
    }
}

pub fn parse(data: &[u8]) -> Result<PolicyFile, String> {
    let mut reader = Reader { data, at: 0 };
    let version = reader.u32()?;
    if version != 0 {
        return Err(format!("aipolicy.data version {version} is not supported (the server reads version 0)"));
    }
    let count = reader.count("policy")?;
    let mut policies = Vec::with_capacity(count);
    for _ in 0..count {
        let at = reader.at;
        let policy_version = reader.u32()?;
        if policy_version != 0 {
            return Err(format!("policy version {policy_version} at {at} (the server reads version 0)"));
        }
        let id = reader.u32()?;
        let triggers = reader.count("trigger")?;
        let mut list = Vec::with_capacity(triggers);
        for _ in 0..triggers {
            list.push(reader.trigger()?);
        }
        policies.push(Policy { version: policy_version, id, triggers: list });
    }
    if reader.at != data.len() {
        return Err(format!("{} bytes follow the last policy", data.len() - reader.at));
    }
    Ok(PolicyFile { version, policies })
}

fn write_condition(out: &mut Vec<u8>, node: &Condition) {
    out.extend_from_slice(&node.kind.to_le_bytes());
    out.extend_from_slice(&(node.param.len() as i32).to_le_bytes());
    out.extend_from_slice(&node.param);
    if node.left.is_none() && node.right.is_none() {
        out.extend_from_slice(&CONDITION_LEAF.to_le_bytes());
        return;
    }
    if let Some(left) = &node.left {
        out.extend_from_slice(&CONDITION_LEFT.to_le_bytes());
        write_condition(out, left);
    }
    if let Some(right) = &node.right {
        out.extend_from_slice(&CONDITION_RIGHT.to_le_bytes());
        write_condition(out, right);
    }
    out.extend_from_slice(&CONDITION_END.to_le_bytes());
}

/// The file as the official editor writes it.
pub fn encode(file: &PolicyFile) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&file.version.to_le_bytes());
    out.extend_from_slice(&(file.policies.len() as i32).to_le_bytes());
    for policy in &file.policies {
        out.extend_from_slice(&policy.version.to_le_bytes());
        out.extend_from_slice(&policy.id.to_le_bytes());
        out.extend_from_slice(&(policy.triggers.len() as i32).to_le_bytes());
        for trigger in &policy.triggers {
            out.extend_from_slice(&trigger.version.to_le_bytes());
            out.extend_from_slice(&trigger.id.to_le_bytes());
            out.push(trigger.active as u8);
            out.push(trigger.run as u8);
            out.push(trigger.run_condition);
            out.extend_from_slice(&trigger.name);
            write_condition(&mut out, &trigger.condition);
            out.extend_from_slice(&(trigger.operations.len() as i32).to_le_bytes());
            for operation in &trigger.operations {
                out.extend_from_slice(&operation.kind.to_le_bytes());
                if matches!(operation.kind, O_TALK | O_WHISPER | O_TALK_PORTRAIT) {
                    out.extend_from_slice(&(operation.param.len() as u32).to_le_bytes());
                }
                out.extend_from_slice(&operation.param);
                out.extend_from_slice(&operation.target.to_le_bytes());
                if let Some(mask) = operation.target_param {
                    out.extend_from_slice(&mask.to_le_bytes());
                }
            }
        }
    }
    out
}

// ── Reading values ──

fn i32_at(bytes: &[u8], at: usize) -> i64 {
    bytes.get(at..at + 4).map_or(0, |slot| i32::from_le_bytes(slot.try_into().unwrap()) as i64)
}

fn u32_at(bytes: &[u8], at: usize) -> i64 {
    bytes.get(at..at + 4).map_or(0, |slot| u32::from_le_bytes(slot.try_into().unwrap()) as i64)
}

fn f32_at(bytes: &[u8], at: usize) -> f64 {
    bytes.get(at..at + 4).map_or(0.0, |slot| f32::from_le_bytes(slot.try_into().unwrap()) as f64)
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|&unit| unit != 0).collect();
    String::from_utf16_lossy(&units)
}

pub fn trigger_name(trigger: &Trigger) -> String {
    let end = trigger.name.iter().position(|&byte| byte == 0).unwrap_or(trigger.name.len());
    GBK.decode(&trigger.name[..end]).0.into_owned()
}

/// How a parameter field is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    U32,
    I32,
    F32,
    /// One byte, 0 or 1.
    Bool,
    /// `WORD[16]` (the summoned monsters' name).
    Name16,
    /// A size-prefixed UTF-16 text with its terminator (talk, whisper, portrait talk).
    Text,
}

/// A parameter field of a condition or operation (`policytype.h`; structs not packed).
pub struct FieldSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub at: usize,
    /// `skill`, `monster`, `item`, `mine`, `task`, `trigger`, `timer`, `path`, `controller`, `global`, `event`,
    /// `attack`, `path_type`.
    pub refers: Option<&'static str>,
}

const fn field(key: &'static str, label: &'static str, kind: FieldKind, at: usize, refers: Option<&'static str>) -> FieldSpec {
    FieldSpec { key, label, kind, at, refers }
}

use FieldKind::{Bool, Name16, Text, F32, I32, U32};

pub fn condition_fields(kind: i32) -> &'static [FieldSpec] {
    const TIMER: &[FieldSpec] = &[field("timer", "timer", U32, 0, Some("timer"))];
    const HP: &[FieldSpec] = &[field("ratio", "ratio", F32, 0, None)];
    const RANDOM: &[FieldSpec] = &[field("chance", "chance", F32, 0, None)];
    const PATH: &[FieldSpec] = &[field("path", "path", U32, 0, Some("path"))];
    const ENMITY: &[FieldSpec] = &[field("players", "more than (players)", I32, 0, None), field("level", "player level (unused)", I32, 4, None)];
    const DISTANCE: &[FieldSpec] = &[field("distance", "distance (m)", F32, 0, None)];
    const SKILL: &[FieldSpec] = &[field("skill", "skill", U32, 0, Some("skill"))];
    const VAR: &[FieldSpec] = &[field("variable", "variable", I32, 0, Some("global"))];
    const CONSTANT: &[FieldSpec] = &[field("value", "value", I32, 0, None)];
    const RANK: &[FieldSpec] = &[field("place", "place", I32, 0, None)];
    match kind {
        C_TIME_COME => TIMER,
        C_HP_LESS => HP,
        C_RANDOM => RANDOM,
        C_PATH_END => PATH,
        C_ENMITY_REACH => ENMITY,
        C_DISTANCE_REACH => DISTANCE,
        C_ATTACK_BY_SKILL => SKILL,
        C_VAR => VAR,
        C_CONSTANT => CONSTANT,
        C_RANK_LEVEL => RANK,
        _ => &[],
    }
}

/// The stored size of a condition's parameter (conditions carry their size, so this is only for new ones).
fn condition_size(kind: i32) -> usize {
    condition_fields(kind).iter().map(|spec| spec.at + 4).max().unwrap_or(0)
}

pub fn operation_fields(kind: i32, version: u32) -> &'static [FieldSpec] {
    const ATTACK: &[FieldSpec] = &[field("type", "type", U32, 0, Some("attack"))];
    const SKILL: &[FieldSpec] = &[field("skill", "skill", U32, 0, Some("skill")), field("level", "level", U32, 4, None)];
    const TEXT: &[FieldSpec] = &[field("text", "text", Text, 0, None)];
    const TRIGGER: &[FieldSpec] = &[field("trigger", "trigger", U32, 0, Some("trigger"))];
    const CREATE_TIMER: &[FieldSpec] = &[field("timer", "timer", U32, 0, Some("timer")), field("period", "period (s)", U32, 4, None), field("times", "times (0 = forever)", U32, 8, None)];
    const KILL_TIMER: &[FieldSpec] = &[field("timer", "timer", U32, 0, Some("timer"))];
    const CONTROLLER: &[FieldSpec] = &[field("controller", "controller", U32, 0, Some("controller")), field("stop", "stop it", Bool, 4, None)];
    const CONTROLLER_V0: &[FieldSpec] = &[field("controller", "controller", U32, 0, Some("controller"))];
    const SUMMON: &[FieldSpec] = &[
        field("monster", "monster", U32, 0, Some("monster")),
        field("looks", "looks like", U32, 4, Some("monster")),
        field("count", "count", U32, 8, None),
        field("life", "lifetime (s, 0 = until killed)", U32, 12, None),
        field("name", "name", Name16, 16, None),
        field("range", "range (m)", F32, 48, None),
        field("follow", "follows", Bool, 52, None),
        field("dispear", "disappears with it", Bool, 53, None),
    ];
    const TASK: &[FieldSpec] = &[field("task", "task", U32, 0, Some("task"))];
    const PATH: &[FieldSpec] = &[field("path", "path", U32, 0, Some("path")), field("type", "type", I32, 4, Some("path_type"))];
    const PATH_V8: &[FieldSpec] = &[field("path", "path", U32, 0, Some("path"))];
    const SNEER: &[FieldSpec] = &[field("range", "range (m)", F32, 0, None)];
    const RANGE_SKILL: &[FieldSpec] = &[field("skill", "skill", U32, 0, Some("skill")), field("level", "level", U32, 4, None), field("range", "range (m)", F32, 8, None)];
    const SET_GLOBAL: &[FieldSpec] = &[field("variable", "variable", I32, 0, Some("global")), field("value", "value", I32, 4, None)];
    const ADD_GLOBAL: &[FieldSpec] = &[field("variable", "variable", I32, 0, Some("global")), field("value", "add", I32, 4, None)];
    const COPY_GLOBAL: &[FieldSpec] = &[field("from", "from", I32, 0, Some("global")), field("to", "to", I32, 4, Some("global"))];
    const MINERAL: &[FieldSpec] = &[
        field("mine", "mine", U32, 0, Some("mine")),
        field("count", "count", U32, 4, None),
        field("hp", "HP", U32, 8, None),
        field("range", "range (m)", F32, 12, None),
        field("bind", "bound to the target", Bool, 16, None),
    ];
    const DROP: &[FieldSpec] = &[field("item", "item", U32, 0, Some("item")), field("count", "count", U32, 4, None), field("expire", "expires (s, 0 = never)", U32, 8, None)];
    const HATE: &[FieldSpec] = &[field("aggro", "aggro", I32, 0, None)];
    const EVENT: &[FieldSpec] = &[field("event", "event", I32, 0, Some("event"))];
    match kind {
        0 => ATTACK,
        1 => SKILL,
        O_TALK | O_WHISPER | O_TALK_PORTRAIT => TEXT,
        O_RUN_TRIGGER | O_STOP_TRIGGER | O_ACTIVE_TRIGGER => TRIGGER,
        O_CREATE_TIMER => CREATE_TIMER,
        O_KILL_TIMER => KILL_TIMER,
        O_ACTIVE_CONTROLLER if version == 0 => CONTROLLER_V0,
        O_ACTIVE_CONTROLLER => CONTROLLER,
        15 => SUMMON,
        16 if version > 0 => TASK,
        O_CHANGE_PATH if version > 0 && version < 9 => PATH_V8,
        O_CHANGE_PATH if version > 0 => PATH,
        19 if version > 0 => SNEER,
        20 => RANGE_SKILL,
        22 if version > 0 => SET_GLOBAL,
        23 if version > 0 => ADD_GLOBAL,
        24 if version > 0 => COPY_GLOBAL,
        25 if version > 0 => MINERAL,
        26 | 30 if version > 0 => DROP,
        27 if version > 0 => HATE,
        28 | 29 if version > 0 => EVENT,
        _ => &[],
    }
}

/// A parameter as the editor shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub value: ParamValue,
    pub refers: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

fn utf16_bytes(text: &str, nul: bool) -> Vec<u8> {
    let mut out: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    if nul {
        out.extend_from_slice(&[0, 0]);
    }
    out
}

fn read_field(spec: &FieldSpec, bytes: &[u8]) -> ParamValue {
    match spec.kind {
        U32 => ParamValue::Int(u32_at(bytes, spec.at)),
        I32 => ParamValue::Int(i32_at(bytes, spec.at)),
        F32 => ParamValue::Float((f32_at(bytes, spec.at) * 1_000_000.0).round() / 1_000_000.0),
        Bool => ParamValue::Bool(bytes.get(spec.at).is_some_and(|&byte| byte != 0)),
        Name16 => ParamValue::Text(utf16(bytes.get(spec.at..spec.at + 32).unwrap_or(&[]))),
        Text => ParamValue::Text(utf16(bytes)),
    }
}

fn decode_params(fields: &'static [FieldSpec], bytes: &[u8]) -> Vec<Param> {
    fields.iter().map(|spec| Param { key: spec.key, label: spec.label, kind: spec.kind, value: read_field(spec, bytes), refers: spec.refers }).collect()
}

fn condition_params(node: &Condition) -> Vec<Param> {
    decode_params(condition_fields(node.kind), &node.param)
}

fn operation_params(operation: &Operation, version: u32) -> Vec<Param> {
    decode_params(operation_fields(operation.kind, version), &operation.param)
}

/// Writes `values` (by key) over `base` (kept where a field is not given or unchanged, so padding and unknown bytes
/// stay), into a parameter of `size` bytes.
fn encode_params(fields: &'static [FieldSpec], size: usize, base: &[u8], values: &HashMap<String, ParamValue>, what: &str) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; size];
    if base.len() == size {
        out.copy_from_slice(base);
    }
    for spec in fields {
        let Some(value) = values.get(spec.key) else { continue };
        if &read_field(spec, &out) == value {
            continue;
        }
        let number = || match value {
            ParamValue::Int(number) => Ok(*number as f64),
            ParamValue::Float(number) => Ok(*number),
            ParamValue::Bool(flag) => Ok(*flag as i64 as f64),
            ParamValue::Text(text) => text.trim().parse::<f64>().map_err(|_| format!("{what}: {} needs a number", spec.label)),
        };
        let whole = |min: f64, max: f64| -> Result<i64, String> {
            let value = number()?;
            if value.fract() != 0.0 || value < min || value > max {
                return Err(format!("{what}: {} must be a whole number from {min} to {max}", spec.label));
            }
            Ok(value as i64)
        };
        let slot = out.get_mut(spec.at..).ok_or_else(|| format!("{what}: {} is outside the parameter", spec.label))?;
        match spec.kind {
            U32 => slot[..4].copy_from_slice(&(whole(0.0, u32::MAX as f64)? as u32).to_le_bytes()),
            I32 => slot[..4].copy_from_slice(&(whole(i32::MIN as f64, i32::MAX as f64)? as i32).to_le_bytes()),
            F32 => {
                let value = number()?;
                if !value.is_finite() {
                    return Err(format!("{what}: {} must be a number", spec.label));
                }
                slot[..4].copy_from_slice(&(value as f32).to_le_bytes());
            }
            Bool => slot[0] = (number()? != 0.0) as u8,
            Name16 => {
                let ParamValue::Text(text) = value else { return Err(format!("{what}: {} needs text", spec.label)) };
                let bytes = utf16_bytes(text, false);
                if bytes.len() > 32 {
                    return Err(format!("{what}: {} has {} characters; at most 16", spec.label, bytes.len() / 2));
                }
                slot[..32].fill(0);
                slot[..bytes.len()].copy_from_slice(&bytes);
            }
            Text => return Err(format!("{what}: texts are written whole")),
        }
    }
    Ok(out)
}

pub fn condition_name(kind: i32) -> &'static str {
    match kind {
        C_TIME_COME => "Timer fires",
        C_HP_LESS => "HP below",
        C_START_ATTACK => "Combat starts",
        C_RANDOM => "Random",
        C_KILL_PLAYER => "Kills its target",
        C_NOT => "Not",
        C_OR => "Or",
        C_AND => "And",
        C_DIED => "Dies",
        C_PATH_END => "Reaches the end of a path",
        C_ENMITY_REACH => "Players in aggro",
        C_DISTANCE_REACH => "Distance from the combat start",
        C_PLUS => "Plus",
        13 => "Minus",
        14 => "Times",
        C_DIVIDE => "Divided by",
        C_GREAT => "Greater than",
        17 => "Less than",
        C_EQU => "Equals",
        C_VAR => "Global variable",
        C_CONSTANT => "Number",
        C_RANK_LEVEL => "Level of a ranking place",
        C_BORN => "Spawns",
        C_ATTACK_BY_SKILL => "Hit by a skill",
        C_RANDOM_SELECTONE => "Pick one operation at random",
        C_LEAVE_COMBAT => "Leaves combat",
        _ => "Unknown condition",
    }
}

pub fn operation_name(kind: i32) -> &'static str {
    match kind {
        0 => "Attack",
        1 => "Use a skill",
        O_TALK => "Talk",
        3 => "Reset the aggro list",
        O_RUN_TRIGGER => "Run a trigger",
        O_STOP_TRIGGER => "Disable a trigger",
        O_ACTIVE_TRIGGER => "Enable a trigger",
        O_CREATE_TIMER => "Start a timer",
        O_KILL_TIMER => "Stop a timer",
        9 => "Flee",
        10 => "Be taunted by the target",
        11 => "Lower the target's aggro to the least",
        12 => "Halve all aggro",
        13 => "Skip the remaining operations",
        O_ACTIVE_CONTROLLER => "Switch a spawn controller",
        15 => "Summon monsters",
        16 => "Trigger a task",
        O_CHANGE_PATH => "Switch path",
        18 => "Disappear",
        19 => "Taunt nearby monsters",
        20 => "Use a range skill",
        21 => "Reset (return home, full HP)",
        22 => "Set a global variable",
        23 => "Add to a global variable",
        24 => "Copy a global variable",
        25 => "Summon mines",
        26 => "Drop items",
        27 => "Change aggro",
        28 => "Start an event",
        29 => "Stop an event",
        30 => "Drop items (scattered)",
        O_WHISPER => "Whisper",
        O_TALK_PORTRAIT => "Talk with a portrait",
        _ => "Unknown operation",
    }
}

pub fn target_name(kind: i32) -> &'static str {
    match kind {
        0 => "Aggro first",
        1 => "Aggro second",
        2 => "A random one after the first",
        3 => "Most HP",
        4 => "Most MP",
        5 => "Least HP",
        T_OCCUPATION_LIST => "Classes",
        7 => "Itself",
        8 => "Everyone in aggro",
        _ => "Unknown target",
    }
}

/// Operations whose target the server uses (others ignore it).
fn needs_target(kind: i32) -> bool {
    matches!(kind, 0 | 1 | O_TALK | 10 | 11 | 27 | O_WHISPER | O_TALK_PORTRAIT | 16 | 15 | 25)
}

/// What decides when a trigger is tested: its root condition (and/or by their left child, not by its child).
pub fn category(node: &Condition) -> &'static str {
    match node.kind {
        C_AND | C_OR => node.left.as_deref().map_or("Heartbeat", category),
        C_NOT => node.right.as_deref().map_or("Heartbeat", category),
        C_TIME_COME => "Timer",
        C_KILL_PLAYER => "Kill",
        C_START_ATTACK => "Combat start",
        C_DIED => "Death",
        C_PATH_END => "Path end",
        C_BORN => "Birth",
        C_ATTACK_BY_SKILL => "Skill hit",
        C_LEAVE_COMBAT => "Leave combat",
        _ => "Heartbeat",
    }
}

/// Whether the trigger disables itself after firing (`IsAutoDisable`).
fn fires_once(node: &Condition) -> bool {
    match node.kind {
        C_AND => node.left.as_deref().is_some_and(fires_once) && node.right.as_deref().is_some_and(fires_once),
        C_OR => node.left.as_deref().is_some_and(fires_once) || node.right.as_deref().is_some_and(fires_once),
        C_NOT => node.right.as_deref().is_some_and(fires_once),
        C_HP_LESS | C_ENMITY_REACH | C_DISTANCE_REACH | C_ATTACK_BY_SKILL => true,
        _ => false,
    }
}

/// Runs one random operation instead of all (`GetExecStrategy`).
fn select_one(node: &Condition) -> bool {
    match node.kind {
        C_AND | C_OR => node.left.as_deref().is_some_and(select_one) || node.right.as_deref().is_some_and(select_one),
        C_NOT => node.right.as_deref().is_some_and(select_one),
        C_RANDOM_SELECTONE => true,
        _ => false,
    }
}

fn percent(value: f64) -> String {
    let shown = (value * 1000.0).round() / 10.0;
    if shown.fract() == 0.0 { format!("{}%", shown as i64) } else { format!("{shown}%") }
}

/// The condition as a readable expression.
pub fn expression(node: &Condition) -> String {
    let side = |child: &Option<Box<Condition>>| child.as_deref().map_or("?".to_string(), expression);
    let wrap = |child: &Option<Box<Condition>>| {
        let text = side(child);
        if child.as_deref().is_some_and(|child| matches!(child.kind, C_AND | C_OR | 12..=18)) { format!("({text})") } else { text }
    };
    let p = &node.param;
    match node.kind {
        C_TIME_COME => format!("timer {} fires", u32_at(p, 0)),
        C_HP_LESS => format!("HP < {}", percent(f32_at(p, 0))),
        C_START_ATTACK => "combat starts".into(),
        C_RANDOM => format!("random {}", percent(f32_at(p, 0))),
        C_KILL_PLAYER => "kills its target".into(),
        C_NOT => format!("NOT {}", wrap(&node.right)),
        C_OR => format!("{} OR {}", wrap(&node.left), wrap(&node.right)),
        C_AND => format!("{} AND {}", wrap(&node.left), wrap(&node.right)),
        C_DIED => "dies".into(),
        C_PATH_END => format!("reaches the end of path {}", u32_at(p, 0)),
        C_ENMITY_REACH => format!("more than {} players in aggro", i32_at(p, 0)),
        C_DISTANCE_REACH => format!("> {} m from the combat start", f32_at(p, 0)),
        12 => format!("{} + {}", wrap(&node.left), wrap(&node.right)),
        13 => format!("{} − {}", wrap(&node.left), wrap(&node.right)),
        14 => format!("{} × {}", wrap(&node.left), wrap(&node.right)),
        C_DIVIDE => format!("{} ÷ {}", wrap(&node.left), wrap(&node.right)),
        C_GREAT => format!("{} > {}", wrap(&node.left), wrap(&node.right)),
        17 => format!("{} < {}", wrap(&node.left), wrap(&node.right)),
        C_EQU => format!("{} = {}", wrap(&node.left), wrap(&node.right)),
        C_VAR => format!("global[{}]", i32_at(p, 0)),
        C_CONSTANT => i32_at(p, 0).to_string(),
        C_RANK_LEVEL => format!("level of ranking place {}", i32_at(p, 0)),
        C_BORN => "spawns".into(),
        C_ATTACK_BY_SKILL => format!("hit by skill {}", u32_at(p, 0)),
        C_RANDOM_SELECTONE => "always (one random operation)".into(),
        C_LEAVE_COMBAT => "leaves combat".into(),
        other => format!("unknown condition {other}"),
    }
}

// ── Views ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionView {
    pub kind: i32,
    pub name: &'static str,
    pub params: Vec<Param>,
    /// The parameter as stored (edits write over it).
    pub raw: Vec<u8>,
    pub left: Option<Box<ConditionView>>,
    pub right: Option<Box<ConditionView>>,
}

fn condition_view(node: &Condition) -> ConditionView {
    ConditionView {
        kind: node.kind,
        name: condition_name(node.kind),
        params: condition_params(node),
        raw: node.param.clone(),
        left: node.left.as_deref().map(|child| Box::new(condition_view(child))),
        right: node.right.as_deref().map(|child| Box::new(condition_view(child))),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationView {
    pub kind: i32,
    pub name: &'static str,
    pub params: Vec<Param>,
    pub raw: Vec<u8>,
    pub target: i32,
    pub target_name: &'static str,
    /// The server uses the target for this operation.
    pub uses_target: bool,
    pub target_mask: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerView {
    pub index: usize,
    pub id: u32,
    pub name: String,
    pub version: u32,
    pub active: bool,
    /// A sub-trigger: only run by another trigger's "run a trigger".
    pub run: bool,
    /// 1: tested only in combat.
    pub battle_only: bool,
    pub category: &'static str,
    pub fires_once: bool,
    pub select_one: bool,
    pub expression: String,
    pub condition: ConditionView,
    pub operations: Vec<OperationView>,
    /// Triggers of this policy that run, enable or disable this one (by ID).
    pub called_by: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyView {
    pub index: usize,
    pub id: u32,
    pub triggers: Vec<TriggerView>,
    /// Another policy before this one has the same ID: the server uses that one.
    pub shadowed_by: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySummary {
    pub index: usize,
    pub id: u32,
    pub triggers: usize,
    pub operations: usize,
    /// Trigger versions used (11, 12).
    pub versions: Vec<u32>,
    pub talks: usize,
    pub shadowed: bool,
    /// Changed since the file was opened or saved.
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub size: usize,
    pub policies: Vec<PolicySummary>,
    pub triggers: usize,
    /// Trigger versions and how many triggers have each.
    pub versions: Vec<(u32, usize)>,
    pub state: EditState,
}

pub struct Document {
    pub path: String,
    pub size: usize,
    pub file: PolicyFile,
    /// The policies as opened or last saved (by ID), to mark changed ones.
    saved: HashMap<u32, Policy>,
    done: Vec<JournalEntry>,
    undone: Vec<JournalEntry>,
    /// Journal length at the last save (none: unreachable by undo/redo).
    saved_at: Option<usize>,
    /// MD5 of the file as read or written (changed-on-disk guard).
    disk: String,
    backed_up: bool,
}

impl Document {
    /// The first policy of an ID (the one the server uses) and its trigger count.
    pub fn policy_by_id(&self, id: u32) -> Option<(usize, usize)> {
        self.file.policies.iter().position(|policy| policy.id == id).map(|index| (index, self.file.policies[index].triggers.len()))
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|error| format!("Could not read {path}: {error}"))?;
        let file = parse(&data).map_err(|error| format!("{path}: {error}"))?;
        let mut saved = HashMap::new();
        for policy in &file.policies {
            saved.entry(policy.id).or_insert_with(|| policy.clone());
        }
        Ok(Document { path: path.to_string(), size: data.len(), file, saved, done: Vec::new(), undone: Vec::new(), saved_at: Some(0), disk: token(&data), backed_up: false })
    }

    fn first_of_ids(&self) -> HashMap<u32, usize> {
        first_of_ids(&self.file.policies)
    }

    pub fn view(&self) -> FileView {
        let first = self.first_of_ids();
        let mut versions: HashMap<u32, usize> = HashMap::new();
        let policies = self
            .file
            .policies
            .iter()
            .enumerate()
            .map(|(index, policy)| {
                let mut used: Vec<u32> = policy.triggers.iter().map(|trigger| trigger.version).collect::<HashSet<_>>().into_iter().collect();
                used.sort_unstable();
                for trigger in &policy.triggers {
                    *versions.entry(trigger.version).or_default() += 1;
                }
                PolicySummary {
                    index,
                    id: policy.id,
                    triggers: policy.triggers.len(),
                    operations: policy.triggers.iter().map(|trigger| trigger.operations.len()).sum(),
                    versions: used,
                    talks: policy.triggers.iter().flat_map(|trigger| &trigger.operations).filter(|operation| matches!(operation.kind, O_TALK | O_WHISPER | O_TALK_PORTRAIT)).count(),
                    shadowed: first.get(&policy.id) != Some(&index),
                    changed: self.saved.get(&policy.id) != Some(policy),
                }
            })
            .collect();
        let mut versions: Vec<(u32, usize)> = versions.into_iter().collect();
        versions.sort_unstable();
        FileView { path: self.path.clone(), size: self.size, policies, triggers: self.file.policies.iter().map(|policy| policy.triggers.len()).sum(), versions, state: self.edit_state() }
    }

    pub fn policy(&self, index: usize) -> Result<PolicyView, String> {
        policy_view(&self.file.policies, index)
    }

    /// Policies matching a search: a number matches policy IDs, trigger IDs and every ID an operation or
    /// condition names (skills, monsters, items, tasks, …); text matches trigger names and talk texts.
    /// `monsters`: policy ID → monster IDs and names (from elements.data), so monsters find their policy.
    pub fn search(&self, query: &str, monsters: &HashMap<u32, Vec<(u32, String)>>) -> Vec<usize> {
        let query = query.trim();
        if query.is_empty() {
            return (0..self.file.policies.len()).collect();
        }
        let number: Option<i64> = query.parse().ok();
        let needle = query.to_lowercase();
        let mut out = Vec::new();
        for (index, policy) in self.file.policies.iter().enumerate() {
            let used_by = monsters.get(&policy.id);
            let hit = number.is_some_and(|number| policy.id as i64 == number || used_by.is_some_and(|list| list.iter().any(|(id, _)| *id as i64 == number)))
                || used_by.is_some_and(|list| list.iter().any(|(_, name)| name.to_lowercase().contains(&needle)))
                || policy.triggers.iter().any(|trigger| {
                    (number.is_some_and(|number| trigger.id as i64 == number))
                        || trigger_name(trigger).to_lowercase().contains(&needle)
                        || condition_has(&trigger.condition, number)
                        || trigger.operations.iter().any(|operation| {
                            operation_params(operation, trigger.version).iter().any(|param| match &param.value {
                                ParamValue::Int(value) => param.refers.is_some() && Some(*value) == number,
                                ParamValue::Text(text) => text.to_lowercase().contains(&needle),
                                _ => false,
                            })
                        })
                });
            if hit {
                out.push(index);
            }
        }
        out
    }
}

/// The first policy of each ID (the one the server keeps).
pub(crate) fn first_of_ids(policies: &[Policy]) -> HashMap<u32, usize> {
    let mut out = HashMap::new();
    for (index, policy) in policies.iter().enumerate() {
    out.entry(policy.id).or_insert(index);
    }
    out
}

pub(crate) fn trigger_view(index: usize, trigger: &Trigger, called_by: Vec<u32>) -> TriggerView {
    TriggerView {
    index,
    id: trigger.id,
    name: trigger_name(trigger),
    version: trigger.version,
    active: trigger.active,
    run: trigger.run,
    battle_only: trigger.run_condition != 0,
    category: category(&trigger.condition),
    fires_once: fires_once(&trigger.condition),
    select_one: select_one(&trigger.condition),
    expression: expression(&trigger.condition),
    condition: condition_view(&trigger.condition),
    operations: trigger
        .operations
        .iter()
        .map(|operation| OperationView {
            kind: operation.kind,
            name: operation_name(operation.kind),
            params: operation_params(operation, trigger.version),
            raw: operation.param.clone(),
            target: operation.target,
            target_name: target_name(operation.target),
            uses_target: needs_target(operation.kind),
            target_mask: operation.target_param,
        })
        .collect(),
    called_by,
    }
}

pub(crate) fn policy_view(policies: &[Policy], index: usize) -> Result<PolicyView, String> {
    let policy = policies.get(index).ok_or_else(|| format!("No policy {}", index + 1))?;
    let mut called_by: HashMap<u32, Vec<u32>> = HashMap::new();
    for trigger in &policy.triggers {
        for operation in &trigger.operations {
            if matches!(operation.kind, O_RUN_TRIGGER | O_STOP_TRIGGER | O_ACTIVE_TRIGGER) {
                let entry = called_by.entry(u32_at(&operation.param, 0) as u32).or_default();
                if !entry.contains(&trigger.id) {
                    entry.push(trigger.id);
                }
            }
        }
    }
    let triggers = policy
        .triggers
        .iter()
        .enumerate()
        .map(|(index, trigger)| trigger_view(index, trigger, called_by.get(&trigger.id).cloned().unwrap_or_default()))
        .collect();
    let shadowed_by = first_of_ids(policies).get(&policy.id).copied().filter(|&first| first != index);
    Ok(PolicyView { index, id: policy.id, triggers, shadowed_by })
}

// ── Editing ──

/// A condition node as the editor sends it: values by parameter key, the stored bytes to write over.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionEdit {
    pub kind: i32,
    #[serde(default)]
    pub values: HashMap<String, ParamValue>,
    #[serde(default)]
    pub raw: Vec<u8>,
    #[serde(default)]
    pub left: Option<Box<ConditionEdit>>,
    #[serde(default)]
    pub right: Option<Box<ConditionEdit>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationEdit {
    pub kind: i32,
    #[serde(default)]
    pub values: HashMap<String, ParamValue>,
    #[serde(default)]
    pub raw: Vec<u8>,
    pub target: i32,
    #[serde(default)]
    pub target_mask: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEdit {
    pub name: String,
    pub active: bool,
    pub run: bool,
    pub battle_only: bool,
    pub condition: ConditionEdit,
    pub operations: Vec<OperationEdit>,
}

/// Arithmetic and values: they make numbers, not conditions (the official editor's `CConditionDlg::Check`).
pub fn is_expression(kind: i32) -> bool {
    matches!(kind, C_PLUS..=C_DIVIDE | C_VAR | C_CONSTANT | C_RANK_LEVEL)
}

/// Condition types the file's server reads (`newer`: a server of version-12 triggers, which reads the newer types in
/// triggers of any version; the 2013 server stops on them).
fn condition_known(kind: i32, newer: bool) -> bool {
    (0..=C_ATTACK_BY_SKILL).contains(&kind) || (newer && matches!(kind, C_RANDOM_SELECTONE | C_LEAVE_COMBAT))
}

fn operation_known(kind: i32, newer: bool) -> bool {
    (0..=29).contains(&kind) || (newer && (30..=O_TALK_PORTRAIT).contains(&kind))
}

fn target_known(kind: i32, newer: bool) -> bool {
    (0..=7).contains(&kind) || (newer && kind == 8)
}

/// Whether a file is for the newer server: it has version-12 triggers or types only that server reads.
pub fn is_newer(file: &PolicyFile) -> bool {
    fn newer_condition(node: &Condition) -> bool {
        matches!(node.kind, C_RANDOM_SELECTONE | C_LEAVE_COMBAT) || node.left.as_deref().is_some_and(newer_condition) || node.right.as_deref().is_some_and(newer_condition)
    }
    file.policies.iter().flat_map(|policy| &policy.triggers).any(|trigger| {
        trigger.version >= 12 || newer_condition(&trigger.condition) || trigger.operations.iter().any(|operation| operation.kind >= 30 || operation.target == 8)
    })
}

/// Builds a condition tree, refusing shapes the server cannot load: and/or need two conditions, not one (on the
/// right), comparisons and arithmetic two values, everything else no children. The 2013 server crashes on a
/// missing child.
fn build_condition(edit: &ConditionEdit, newer: bool, depth: usize) -> Result<Condition, String> {
    if depth > 32 {
        return Err("The condition is nested too deeply".into());
    }
    let name = condition_name(edit.kind);
    if !condition_known(edit.kind, newer) {
        return Err(format!("{name} ({}) is a condition only newer servers read; this file is for the 2013 server (version-11 triggers)", edit.kind));
    }
    let child = |side: &Option<Box<ConditionEdit>>, value: bool, which: &str| -> Result<Option<Box<Condition>>, String> {
        let node = side.as_deref().ok_or_else(|| format!("{name} needs a {which} part"))?;
        if is_expression(node.kind) != value {
            return Err(format!("The {which} part of {name} must be {}", if value { "a number (a variable, number, ranking level or arithmetic)" } else { "a condition, not a number" }));
        }
        Ok(Some(Box::new(build_condition(node, newer, depth + 1)?)))
    };
    let (left, right) = match edit.kind {
        C_AND | C_OR => (child(&edit.left, false, "left")?, child(&edit.right, false, "right")?),
        C_NOT => {
            if edit.left.is_some() {
                return Err("Not has only one part".into());
            }
            (None, child(&edit.right, false, "inner")?)
        }
        C_PLUS..=C_DIVIDE | C_GREAT..=C_EQU => (child(&edit.left, true, "left")?, child(&edit.right, true, "right")?),
        _ => {
            if edit.left.is_some() || edit.right.is_some() {
                return Err(format!("{name} has no parts"));
            }
            (None, None)
        }
    };
    let size = if edit.raw.len() == condition_size(edit.kind) || edit.raw.is_empty() { condition_size(edit.kind) } else { edit.raw.len().max(condition_size(edit.kind)) };
    let param = encode_params(condition_fields(edit.kind), size, &edit.raw, &edit.values, name)?;
    Ok(Condition { kind: edit.kind, param, left, right })
}

fn build_operation(edit: &OperationEdit, version: u32, newer: bool, number: usize) -> Result<Operation, String> {
    let name = format!("Operation {number} ({})", operation_name(edit.kind));
    if !operation_known(edit.kind, newer) {
        return Err(format!("{name}: an operation only newer servers read; this file is for the 2013 server (version-11 triggers)"));
    }
    if !target_known(edit.target, newer) {
        return Err(format!("{name}: target {} only newer servers read; this file is for the 2013 server", edit.target));
    }
    let param = if matches!(edit.kind, O_TALK | O_WHISPER | O_TALK_PORTRAIT) {
        let text = match edit.values.get("text") {
            Some(ParamValue::Text(text)) => text.clone(),
            None => utf16(&edit.raw),
            Some(_) => return Err(format!("{name}: the text must be text")),
        };
        if text.encode_utf16().count() > 2000 {
            return Err(format!("{name}: the text is longer than 2,000 characters"));
        }
        // Unchanged texts keep their stored bytes; new ones end with a terminator, as all official texts do.
        if !edit.raw.is_empty() && utf16(&edit.raw) == text { edit.raw.clone() } else { utf16_bytes(&text, true) }
    } else {
        let size = operation_size(edit.kind, version)?.unwrap_or(0);
        encode_params(operation_fields(edit.kind, version), size, &edit.raw, &edit.values, &name)?
    };
    let target_param = (edit.target == T_OCCUPATION_LIST).then(|| edit.target_mask.unwrap_or(0));
    Ok(Operation { kind: edit.kind, param, target: edit.target, target_param })
}

/// A trigger from an edit, keeping the stored name slot and run-condition byte while they say the same.
pub fn build_trigger(old: &Trigger, edit: &TriggerEdit, newer: bool) -> Result<Trigger, String> {
    if is_expression(edit.condition.kind) {
        return Err("A trigger's condition must be a condition, not a number".into());
    }
    let name = if edit.name == trigger_name(old) {
        old.name.clone()
    } else {
        let (encoded, _, unmappable) = GBK.encode(&edit.name);
        if unmappable {
            return Err("The name has characters GBK cannot store".into());
        }
        if encoded.len() > 127 {
            return Err(format!("The name is {} bytes in GBK; at most 127", encoded.len()));
        }
        let mut slot = vec![0u8; 128];
        slot[..encoded.len()].copy_from_slice(&encoded);
        slot
    };
    let run_condition = if (old.run_condition != 0) == edit.battle_only { old.run_condition } else { edit.battle_only as u8 };
    let condition = build_condition(&edit.condition, newer, 0)?;
    let operations = edit.operations.iter().enumerate().map(|(index, operation)| build_operation(operation, old.version, newer, index + 1)).collect::<Result<Vec<_>, _>>()?;
    Ok(Trigger { version: old.version, id: old.id, active: edit.active, run: edit.run, run_condition, name, condition, operations })
}

/// A trigger or policy action from the lists.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ListAction {
    Clone,
    Delete,
    Move { to: usize },
}

#[derive(Debug, Clone)]
struct PolicyChange {
    index: usize,
    before: Option<Policy>,
    after: Option<Policy>,
}

#[derive(Debug, Clone)]
struct JournalEntry {
    label: String,
    time: i64,
    changes: Vec<PolicyChange>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub label: String,
    pub time: i64,
    pub undone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub history: Vec<HistoryRow>,
}

/// What an edit changed: the editing state, the policy to show, and the list when policies moved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditResult {
    pub state: EditState,
    pub policy: Option<PolicyView>,
    pub file: Option<FileView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub backup: Option<String>,
}

fn token(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Document {
    pub fn edit_state(&self) -> EditState {
        let mut history: Vec<HistoryRow> = self.done.iter().map(|entry| HistoryRow { label: entry.label.clone(), time: entry.time, undone: false }).collect();
        history.extend(self.undone.iter().rev().map(|entry| HistoryRow { label: entry.label.clone(), time: entry.time, undone: true }));
        EditState { dirty: self.saved_at != Some(self.done.len()), can_undo: !self.done.is_empty(), can_redo: !self.undone.is_empty(), history }
    }

    fn apply(&mut self, change: &PolicyChange, forward: bool) {
        let (from, to) = if forward { (&change.before, &change.after) } else { (&change.after, &change.before) };
        match (from, to) {
            (Some(_), Some(policy)) => self.file.policies[change.index] = policy.clone(),
            (None, Some(policy)) => self.file.policies.insert(change.index, policy.clone()),
            (Some(_), None) => {
                self.file.policies.remove(change.index);
            }
            (None, None) => {}
        }
    }

    fn record(&mut self, label: String, changes: Vec<PolicyChange>) {
        for change in &changes {
            self.apply(change, true);
        }
        if self.saved_at.is_some_and(|saved| saved > self.done.len()) {
            self.saved_at = None;
        }
        self.undone.clear();
        self.done.push(JournalEntry { label, time: chrono::Local::now().timestamp(), changes });
    }

    fn result(&self, policy: Option<usize>, list_changed: bool) -> Result<EditResult, String> {
        Ok(EditResult { state: self.edit_state(), policy: policy.filter(|&index| index < self.file.policies.len()).map(|index| self.policy(index)).transpose()?, file: list_changed.then(|| self.view()) })
    }

    /// Replaces a trigger with an edit (one undo step).
    pub fn set_trigger(&mut self, policy: usize, trigger: usize, edit: &TriggerEdit) -> Result<EditResult, String> {
        let before = self.file.policies.get(policy).cloned().ok_or_else(|| format!("No policy {}", policy + 1))?;
        let old = before.triggers.get(trigger).ok_or_else(|| format!("No trigger {}", trigger + 1))?;
        let new = build_trigger(old, edit, is_newer(&self.file))?;
        if &new == old {
            return self.result(Some(policy), false);
        }
        let mut after = before.clone();
        after.triggers[trigger] = new;
        self.record(format!("Edit trigger {} of policy {}", old.id, before.id), vec![PolicyChange { index: policy, before: Some(before), after: Some(after) }]);
        self.result(Some(policy), false)
    }

    /// Clones (below, with the next free ID), deletes or moves a trigger.
    pub fn trigger_action(&mut self, policy: usize, trigger: usize, action: &ListAction) -> Result<EditResult, String> {
        let before = self.file.policies.get(policy).cloned().ok_or_else(|| format!("No policy {}", policy + 1))?;
        let source = before.triggers.get(trigger).cloned().ok_or_else(|| format!("No trigger {}", trigger + 1))?;
        let mut after = before.clone();
        let label = match action {
            ListAction::Clone => {
                let mut copy = source.clone();
                copy.id = before.triggers.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
                after.triggers.insert(trigger + 1, copy);
                format!("Clone trigger {} of policy {}", source.id, before.id)
            }
            ListAction::Delete => {
                after.triggers.remove(trigger);
                format!("Delete trigger {} of policy {}", source.id, before.id)
            }
            ListAction::Move { to } => {
                if *to >= before.triggers.len() || *to == trigger {
                    return self.result(Some(policy), false);
                }
                let moved = after.triggers.remove(trigger);
                after.triggers.insert(*to, moved);
                format!("Move trigger {} of policy {}", source.id, before.id)
            }
        };
        self.record(label, vec![PolicyChange { index: policy, before: Some(before), after: Some(after) }]);
        self.result(Some(policy), true)
    }

    /// Clones (below, with the next free ID) or deletes a policy.
    pub fn policy_action(&mut self, policy: usize, action: &ListAction) -> Result<EditResult, String> {
        let source = self.file.policies.get(policy).cloned().ok_or_else(|| format!("No policy {}", policy + 1))?;
        match action {
            ListAction::Clone => {
                let mut copy = source.clone();
                copy.id = self.file.policies.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
                let id = copy.id;
                self.record(format!("Clone policy {} as {id}", source.id), vec![PolicyChange { index: policy + 1, before: None, after: Some(copy) }]);
                self.result(Some(policy + 1), true)
            }
            ListAction::Delete => {
                self.record(format!("Delete policy {}", source.id), vec![PolicyChange { index: policy, before: Some(source), after: None }]);
                self.result(None, true)
            }
            ListAction::Move { .. } => Err("Policies keep their order".into()),
        }
    }

    /// Sub-triggers no top-level trigger reaches through "run a trigger" (the official editor's `DelRedundancy`),
    /// per policy: (policy position, trigger IDs).
    pub fn unused_subtriggers(&self, policy: Option<usize>) -> Vec<(usize, Vec<u32>)> {
        let positions: Vec<usize> = match policy {
            Some(index) => vec![index],
            None => (0..self.file.policies.len()).collect(),
        };
        positions
            .into_iter()
            .filter_map(|index| {
                let policy = self.file.policies.get(index)?;
                let by_id: HashMap<u32, &Trigger> = policy.triggers.iter().rev().map(|trigger| (trigger.id, trigger)).collect();
                let mut reached = HashSet::new();
                let mut stack: Vec<u32> = policy.triggers.iter().filter(|trigger| !trigger.run).map(|trigger| trigger.id).collect();
                while let Some(id) = stack.pop() {
                    if !reached.insert(id) {
                        continue;
                    }
                    if let Some(trigger) = by_id.get(&id) {
                        stack.extend(trigger.operations.iter().filter(|operation| operation.kind == O_RUN_TRIGGER).map(|operation| u32_at(&operation.param, 0) as u32));
                    }
                }
                let unused: Vec<u32> = policy.triggers.iter().filter(|trigger| trigger.run && !reached.contains(&trigger.id)).map(|trigger| trigger.id).collect();
                (!unused.is_empty()).then_some((index, unused))
            })
            .collect()
    }

    /// Removes the sub-triggers nothing runs from one policy or all (one undo step); `shown` is the policy to return.
    pub fn cleanup(&mut self, policy: Option<usize>, shown: Option<usize>) -> Result<EditResult, String> {
        let unused = self.unused_subtriggers(policy);
        if unused.is_empty() {
            return self.result(policy.or(shown), false);
        }
        let mut removed = 0;
        let changes: Vec<PolicyChange> = unused
            .iter()
            .map(|(index, ids)| {
                let before = self.file.policies[*index].clone();
                let mut after = before.clone();
                after.triggers.retain(|trigger| !(trigger.run && ids.contains(&trigger.id)));
                removed += before.triggers.len() - after.triggers.len();
                PolicyChange { index: *index, before: Some(before), after: Some(after) }
            })
            .collect();
        let label = match policy {
            Some(index) => format!("Remove {removed} unused sub-trigger{} of policy {}", if removed == 1 { "" } else { "s" }, self.file.policies[index].id),
            None => format!("Remove {removed} unused sub-trigger{} from {} policies", if removed == 1 { "" } else { "s" }, changes.len()),
        };
        self.record(label, changes);
        self.result(policy.or(shown), true)
    }

    /// The policy the last step changed (to show after undo/redo).
    fn touched(entry: &JournalEntry) -> Option<usize> {
        entry.changes.iter().find(|change| change.after.is_some() || change.before.is_some()).map(|change| change.index)
    }

    pub fn undo(&mut self) -> Result<EditResult, String> {
        let entry = self.done.pop().ok_or("Nothing to undo")?;
        for change in entry.changes.iter().rev() {
            self.apply(change, false);
        }
        let shown = Self::touched(&entry);
        self.undone.push(entry);
        self.result(shown, true)
    }

    pub fn redo(&mut self) -> Result<EditResult, String> {
        let entry = self.undone.pop().ok_or("Nothing to redo")?;
        for change in &entry.changes {
            self.apply(change, true);
        }
        let shown = Self::touched(&entry);
        self.done.push(entry);
        self.result(shown, true)
    }

    /// Writes the file: it must read back as exactly these policies; a 7z backup on the first save over a file;
    /// refuses when another program changed the file since it was read (unless `replace_changed`).
    pub fn save(&mut self, target: Option<&str>, backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
        let path = std::path::PathBuf::from(target.unwrap_or(&self.path));
        let same = crate::path_data::same_path(&path, std::path::Path::new(&self.path));
        if same && !replace_changed {
            if let Ok(current) = std::fs::read(&path) {
                if token(&current) != self.disk {
                    return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", path.display()));
                }
            }
        }
        let data = encode(&self.file);
        if parse(&data)? != self.file {
            return Err("The written file would not read back as these policies; nothing was saved".into());
        }
        let backup = if backup && path.is_file() && !(same && self.backed_up) { Some(crate::backup::archive(&path, std::slice::from_ref(&path))?) } else { None };
        crate::path_data::write_replacing(&path, &data)?;
        if same && backup.is_some() {
            self.backed_up = true;
        }
        self.path = path.display().to_string();
        self.size = data.len();
        self.disk = token(&data);
        self.saved_at = Some(self.done.len());
        self.saved = HashMap::new();
        for policy in &self.file.policies {
            self.saved.entry(policy.id).or_insert_with(|| policy.clone());
        }
        Ok(SaveReport { path: self.path.clone(), size: data.len(), backup: backup.map(|path| path.display().to_string()) })
    }
}

// ── Catalog ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub refers: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeInfo {
    pub kind: i32,
    pub name: &'static str,
    pub fields: Vec<FieldInfo>,
    /// Conditions: a number (variable, number, ranking level, arithmetic) rather than a condition.
    pub expression: bool,
    /// Conditions: and/or/not or a comparison/arithmetic (they have parts).
    pub parts: u8,
    /// Operations: the server uses the target.
    pub uses_target: bool,
    /// Only newer servers read it.
    pub newer: bool,
}

/// The types the editor offers for this file, with their parameter fields.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCatalog {
    pub newer: bool,
    pub conditions: Vec<TypeInfo>,
    pub operations: Vec<TypeInfo>,
    pub targets: Vec<(i32, &'static str)>,
}

fn field_infos(fields: &'static [FieldSpec]) -> Vec<FieldInfo> {
    fields.iter().map(|spec| FieldInfo { key: spec.key, label: spec.label, kind: spec.kind, refers: spec.refers }).collect()
}

pub fn catalog(newer: bool) -> TypeCatalog {
    let conditions = (0..=C_LEAVE_COMBAT)
        .filter(|&kind| condition_known(kind, newer))
        .map(|kind| TypeInfo {
            kind,
            name: condition_name(kind),
            fields: field_infos(condition_fields(kind)),
            expression: is_expression(kind),
            parts: match kind {
                C_NOT => 1,
                C_AND | C_OR | C_PLUS..=C_DIVIDE | C_GREAT..=C_EQU => 2,
                _ => 0,
            },
            uses_target: false,
            newer: !condition_known(kind, false),
        })
        .collect();
    let operations = (0..=O_TALK_PORTRAIT)
        .filter(|&kind| operation_known(kind, newer))
        .map(|kind| TypeInfo { kind, name: operation_name(kind), fields: field_infos(operation_fields(kind, 12)), expression: false, parts: 0, uses_target: needs_target(kind), newer: !operation_known(kind, false) })
        .collect();
    let targets = (0..=8).filter(|&kind| target_known(kind, newer)).map(|kind| (kind, target_name(kind))).collect();
    TypeCatalog { newer, conditions, operations, targets }
}

// ── Problems ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `error`: a server would crash or refuse; `warning`: it misbehaves; `note`: probably unintended.
    pub severity: &'static str,
    /// Positions of the policy and trigger (none: the file).
    pub policy: Option<usize>,
    pub trigger: Option<usize>,
    /// Their IDs, for the list.
    pub policy_id: Option<u32>,
    pub trigger_id: Option<u32>,
    pub message: String,
}

/// What the other files know, for reference checks (none: that file is not open).
#[derive(Default)]
pub struct ProblemContext {
    pub skills: Option<HashSet<u32>>,
    /// Essence IDs (monsters, items, mines) of the open elements.data.
    pub elements: Option<HashSet<u32>>,
    pub tasks: Option<HashSet<u32>>,
    /// Records of elements.data by the policy they name.
    pub monsters: HashMap<u32, Vec<(u32, String)>>,
}

/// Event conditions: they decide when a trigger is tested, so they only work as its first condition. Elsewhere combat
/// start, kill, death, birth and leave combat are always true, path end and skill hit never (timers work anywhere:
/// heartbeat triggers are tested in the tick a timer fires).
fn is_event(kind: i32) -> bool {
    matches!(kind, C_START_ATTACK | C_KILL_PLAYER | C_DIED | C_PATH_END | C_BORN | C_ATTACK_BY_SKILL | C_LEAVE_COMBAT)
}

/// Events below the path that sets the trigger's category (and/or: the left child; not: its child).
fn misplaced_events(node: &Condition, on_path: bool, out: &mut Vec<i32>) {
    if is_event(node.kind) && !on_path {
        out.push(node.kind);
    }
    match node.kind {
        C_AND | C_OR => {
            if let Some(left) = &node.left {
                misplaced_events(left, on_path, out);
            }
            if let Some(right) = &node.right {
                misplaced_events(right, false, out);
            }
        }
        C_NOT => {
            if let Some(right) = &node.right {
                misplaced_events(right, on_path, out);
            }
        }
        _ => {
            for child in [&node.left, &node.right].into_iter().flatten() {
                misplaced_events(child, false, out);
            }
        }
    }
}

fn condition_ids(node: &Condition, refers: &str, out: &mut Vec<i64>) {
    for param in condition_params(node) {
        if param.refers == Some(refers) {
            if let ParamValue::Int(value) = param.value {
                out.push(value);
            }
        }
    }
    for child in [&node.left, &node.right].into_iter().flatten() {
        condition_ids(child, refers, out);
    }
}

fn operation_ids(trigger: &Trigger, refers: &[&str]) -> Vec<(i64, &'static str)> {
    trigger
        .operations
        .iter()
        .flat_map(|operation| operation_params(operation, trigger.version))
        .filter_map(|param| match (param.refers, param.value) {
            (Some(kind), ParamValue::Int(value)) if refers.contains(&kind) => Some((value, kind)),
            _ => None,
        })
        .collect()
}

impl Document {
    /// IDs of skills, elements.data records (monsters, items, mines) and tasks the policies name.
    pub fn referenced_ids(&self) -> (HashSet<u32>, HashSet<u32>, HashSet<u32>) {
        let (mut skills, mut elements, mut tasks) = (HashSet::new(), HashSet::new(), HashSet::new());
        for trigger in self.file.policies.iter().flat_map(|policy| &policy.triggers) {
            let mut in_conditions = Vec::new();
            condition_ids(&trigger.condition, "skill", &mut in_conditions);
            skills.extend(in_conditions.into_iter().filter_map(|id| u32::try_from(id).ok()));
            for (id, kind) in operation_ids(trigger, &["skill", "monster", "item", "mine", "task"]) {
                let Ok(id) = u32::try_from(id) else { continue };
                if id == 0 {
                    continue;
                }
                match kind {
                    "skill" => skills.insert(id),
                    "task" => tasks.insert(id),
                    _ => elements.insert(id),
                };
            }
        }
        (skills, elements, tasks)
    }

    pub fn problems(&self, context: &ProblemContext) -> Vec<Problem> {
        let mut out = Vec::new();
        // The newest trigger version tells which server the file is for (11: the 2013 source).
        let newer = is_newer(&self.file);
        let first = self.first_of_ids();
        for (index, policy) in self.file.policies.iter().enumerate() {
            let mut add = |severity, trigger: Option<usize>, message: String| out.push(Problem { severity, policy: Some(index), trigger, policy_id: Some(policy.id), trigger_id: trigger.map(|at| policy.triggers[at].id), message });
            if first.get(&policy.id) != Some(&index) {
                add("warning", None, format!("Policy ID {} is also policy #{}: the server keeps the first and ignores this one", policy.id, first[&policy.id] + 1));
            }
            let ids: HashMap<u32, usize> = policy.triggers.iter().enumerate().map(|(position, trigger)| (trigger.id, position)).rev().collect();
            let mut seen = HashSet::new();
            // Timers: started by operations, tested by conditions.
            let mut started = HashSet::new();
            let mut tested: Vec<(usize, i64)> = Vec::new();
            // Triggers run, enabled or disabled by others.
            let mut ran = HashSet::new();
            let mut enabled = HashSet::new();
            for (position, trigger) in policy.triggers.iter().enumerate() {
                if !seen.insert(trigger.id) {
                    add("warning", Some(position), format!("Trigger ID {} is used twice in this policy: running it finds the first", trigger.id));
                }
                for operation in &trigger.operations {
                    let id = u32_at(&operation.param, 0);
                    match operation.kind {
                        O_CREATE_TIMER => {
                            started.insert(id);
                        }
                        O_RUN_TRIGGER => {
                            ran.insert(id as u32);
                        }
                        O_ACTIVE_TRIGGER => {
                            enabled.insert(id as u32);
                        }
                        _ => {}
                    }
                }
                let mut timers = Vec::new();
                condition_ids(&trigger.condition, "timer", &mut timers);
                tested.extend(timers.into_iter().map(|timer| (position, timer)));
            }
            for (position, trigger) in policy.triggers.iter().enumerate() {
                let label = |kind: i32| condition_name(kind).to_lowercase();
                let mut events = Vec::new();
                misplaced_events(&trigger.condition, true, &mut events);
                for kind in events {
                    let effect = if matches!(kind, C_PATH_END | C_ATTACK_BY_SKILL) { "never true" } else { "always true" };
                    add("warning", Some(position), format!("\"{}\" only works as the trigger's first condition (it decides when the trigger is tested); here it is {effect}", label(kind)));
                }
                if trigger.operations.is_empty() {
                    if trigger.run {
                        add("note", Some(position), "The sub-trigger has no operations".into());
                    } else {
                        // policy::ResetAll (death) and Reset (leaving combat, battle triggers) call _ops->Reset() on a null list.
                        add("error", Some(position), format!("The trigger has no operations: the server crashes when the monster dies{}", if trigger.run_condition != 0 { " or leaves combat" } else { "" }));
                    }
                }
                for operation in &trigger.operations {
                    let id = u32_at(&operation.param, 0) as u32;
                    match operation.kind {
                        O_RUN_TRIGGER if !ids.contains_key(&id) => add(
                            if newer { "warning" } else { "error" },
                            Some(position),
                            if newer { format!("Runs trigger {id}, which this policy does not have: nothing happens") } else { format!("Runs trigger {id}, which this policy does not have: the 2013 server crashes while loading") },
                        ),
                        O_STOP_TRIGGER | O_ACTIVE_TRIGGER if !ids.contains_key(&id) => add("warning", Some(position), format!("{} trigger {id}, which this policy does not have: nothing happens", if operation.kind == O_ACTIVE_TRIGGER { "Enables" } else { "Disables" })),
                        20 if !newer => add("error", Some(position), "\"Use a range skill\": the 2013 server stops on an assertion when it loads it".into()),
                        O_TALK | O_WHISPER | O_TALK_PORTRAIT if utf16(&operation.param).trim().is_empty() => add("warning", Some(position), format!("{} with an empty text", operation_name(operation.kind))),
                        _ => {}
                    }
                }
                // References other files can check.
                let mut skills = Vec::new();
                condition_ids(&trigger.condition, "skill", &mut skills);
                let mut named: Vec<(i64, &str)> = skills.into_iter().map(|id| (id, "skill")).collect();
                named.extend(operation_ids(trigger, &["skill", "monster", "item", "mine", "task"]));
                for (id, kind) in named {
                    let Ok(id) = u32::try_from(id) else { continue };
                    if id == 0 {
                        continue;
                    }
                    let known = match kind {
                        "skill" => context.skills.as_ref(),
                        "task" => context.tasks.as_ref(),
                        _ => context.elements.as_ref(),
                    };
                    if known.is_some_and(|known| !known.contains(&id)) {
                        let source = match kind {
                            "skill" => "the client's skills",
                            "task" => "the open tasks.data",
                            _ => "the open elements.data",
                        };
                        add("warning", Some(position), format!("{} {id} is not in {source}", match kind { "skill" => "Skill", "task" => "Task", "monster" => "Monster", "item" => "Item", _ => "Mine" }));
                    }
                }
                if trigger.run && !ran.contains(&trigger.id) {
                    add("note", Some(position), "A sub-trigger nothing in this policy runs: it never runs".into());
                }
                let forced = matches!(category(&trigger.condition), "Combat start" | "Death" | "Path end" | "Birth" | "Skill hit");
                if !trigger.run && !trigger.active && !forced && !enabled.contains(&trigger.id) {
                    add("note", Some(position), "Disabled at start and nothing enables it: it never runs".into());
                }
            }
            for (position, timer) in tested {
                if !started.contains(&timer) {
                    add("warning", Some(position), format!("Tests timer {timer}, which nothing in this policy starts: it never fires"));
                }
            }
            // "Run a trigger" loads the trigger it runs into the caller, so a loop never finishes loading.
            for (position, trigger) in policy.triggers.iter().enumerate() {
                let mut stack = vec![(trigger.id, 0usize)];
                let mut visited = HashSet::new();
                let mut looped = false;
                while let Some((id, depth)) = stack.pop() {
                    if depth > 64 || !visited.insert(id) {
                        continue;
                    }
                    let Some(&at) = ids.get(&id) else { continue };
                    for operation in &policy.triggers[at].operations {
                        if operation.kind == O_RUN_TRIGGER {
                            let next = u32_at(&operation.param, 0) as u32;
                            if next == trigger.id {
                                looped = true;
                            }
                            stack.push((next, depth + 1));
                        }
                    }
                }
                if looped {
                    add("error", Some(position), "Runs itself (directly or through other triggers): the server never finishes loading the policy".into());
                }
            }
        }
        // Records naming policies the file does not have.
        let mut missing: Vec<(&u32, &Vec<(u32, String)>)> = context.monsters.iter().filter(|(id, _)| !first.contains_key(id)).collect();
        missing.sort_by_key(|(id, _)| **id);
        for (id, records) in missing {
            let names: Vec<String> = records.iter().take(4).map(|(record, name)| format!("{name} ({record})")).collect();
            out.push(Problem {
                severity: "warning",
                policy: None,
                trigger: None,
                policy_id: Some(*id),
                trigger_id: None,
                message: format!("Policy {id} is not in the file but {} name{} it: {}{}; the server gives them no policy", records.len(), if records.len() == 1 { "s" } else { "" }, names.join(", "), if records.len() > 4 { ", …" } else { "" }),
            });
        }
        out
    }
}

fn condition_has(node: &Condition, number: Option<i64>) -> bool {
    let Some(number) = number else { return false };
    condition_params(node).iter().any(|param| param.refers.is_some() && matches!(param.value, ParamValue::Int(value) if value == number))
        || node.left.as_deref().is_some_and(|child| condition_has(child, Some(number)))
        || node.right.as_deref().is_some_and(|child| condition_has(child, Some(number)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<String> {
        [
            "E:/Games/XtremeJade/element/data/aipolicy.data",
            "E:/Games/ForsakenJD/element/data/aipolicy.data",
            "E:/Game Dev/JD/1559/gamed/config/aipolicy.data",
            "E:/Game Dev/JD/zxserver/zgame/gs/config/aipolicy.data",
            "E:/Game Dev/JD/zx_18_compiled/gamed/config/aipolicy.data",
            "E:/Game Dev/JD/zx_18_compiled/gamed/config/aipolicy.data-cn",
        ]
        .iter()
        .map(|path| path.to_string())
        .filter(|path| std::path::Path::new(path).is_file())
        .collect()
    }

    #[test]
    fn samples_read_and_write_back_byte_for_byte() {
        for path in samples() {
            let data = std::fs::read(&path).unwrap();
            let file = parse(&data).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(file.policies.len() > 2000, "{path}");
            assert_eq!(encode(&file), data, "{path}");
        }
    }

    #[test]
    fn monsters_link_through_the_aipolicy_role() {
        let elements = "E:/Games/ForsakenJD/element/data/elements.data";
        let policies = "E:/Games/ForsakenJD/element/data/aipolicy.data";
        if !std::path::Path::new(elements).is_file() || !std::path::Path::new(policies).is_file() {
            return;
        }
        let document = crate::elements::Document::open(elements.to_string(), std::sync::Arc::new(crate::elements::format::Catalog::load(None))).unwrap();
        let used = document.monsters_by_policy();
        let file = Document::open(policies).unwrap();
        let monsters: usize = used.values().map(Vec::len).sum();
        let known = used.iter().filter(|(id, _)| file.policy_by_id(**id).is_some()).map(|(_, list)| list.len()).sum::<usize>();
        // The role sits on MONSTER_ESSENCE.common_strategy (v160: AIPolicy): thousands of monsters, nearly all
        // naming a policy the file has (id_strategy, the attack strategy, would be 1–6).
        assert!(monsters > 4000, "{monsters}");
        assert!(known * 100 >= monsters * 99, "{known} of {monsters}");
        assert!(used.keys().any(|&id| id > 100));
    }

    fn edit_of_condition(view: &ConditionView) -> ConditionEdit {
        ConditionEdit {
            kind: view.kind,
            values: view.params.iter().map(|param| (param.key.to_string(), param.value.clone())).collect(),
            raw: view.raw.clone(),
            left: view.left.as_deref().map(|child| Box::new(edit_of_condition(child))),
            right: view.right.as_deref().map(|child| Box::new(edit_of_condition(child))),
        }
    }

    /// The edit the UI sends for a trigger it shows unchanged.
    fn edit_of(view: &TriggerView) -> TriggerEdit {
        TriggerEdit {
            name: view.name.clone(),
            active: view.active,
            run: view.run,
            battle_only: view.battle_only,
            condition: edit_of_condition(&view.condition),
            operations: view
                .operations
                .iter()
                .map(|operation| OperationEdit { kind: operation.kind, values: operation.params.iter().map(|param| (param.key.to_string(), param.value.clone())).collect(), raw: operation.raw.clone(), target: operation.target, target_mask: operation.target_mask })
                .collect(),
        }
    }

    #[test]
    fn every_trigger_rebuilds_from_its_view_unchanged() {
        for path in samples() {
            let document = Document::open(&path).unwrap();
            for index in 0..document.file.policies.len() {
                let view = document.policy(index).unwrap();
                for (position, trigger) in view.triggers.iter().enumerate() {
                    let original = &document.file.policies[index].triggers[position];
                    let rebuilt = build_trigger(original, &edit_of(trigger), is_newer(&document.file)).unwrap_or_else(|error| panic!("{path} policy {} trigger {}: {error}", view.id, trigger.id));
                    assert_eq!(&rebuilt, original, "{path} policy {} trigger {}", view.id, trigger.id);
                }
            }
        }
    }

    #[test]
    fn edits_check_shapes_save_and_undo() {
        let Some(source) = samples().into_iter().nth(1) else { return };
        let folder = std::env::temp_dir().join(format!("jdide-aipolicy-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("aipolicy.data");
        std::fs::copy(&source, &path).unwrap();
        let mut document = Document::open(path.to_str().unwrap()).unwrap();
        // A policy with a talk and an HP condition.
        let (policy, trigger) = (0..document.file.policies.len())
            .flat_map(|index| (0..document.file.policies[index].triggers.len()).map(move |position| (index, position)))
            .find(|&(index, position)| {
                let trigger = &document.file.policies[index].triggers[position];
                trigger.condition.kind == C_HP_LESS && trigger.operations.iter().any(|operation| operation.kind == O_TALK)
            })
            .unwrap();
        let view = document.policy(policy).unwrap().triggers[trigger].clone();
        let mut edit = edit_of(&view);
        edit.condition.values.insert("ratio".into(), ParamValue::Float(0.25));
        let talk = edit.operations.iter().position(|operation| operation.kind == O_TALK).unwrap();
        edit.operations[talk].values.insert("text".into(), ParamValue::Text("$BEdited text".into()));
        edit.name = "Edited".into();
        let result = document.set_trigger(policy, trigger, &edit).unwrap();
        assert!(result.state.dirty && result.state.can_undo);
        let changed = &document.file.policies[policy].triggers[trigger];
        assert_eq!(expression(&changed.condition), "HP < 25%");
        assert_eq!(utf16(&changed.operations[talk].param), "$BEdited text");
        assert!(changed.operations[talk].param.ends_with(&[0, 0]), "texts end with a terminator");
        assert_eq!(trigger_name(changed), "Edited");

        // Shapes the server cannot load are refused.
        let mut bad = edit.clone();
        bad.condition = ConditionEdit { kind: C_AND, values: HashMap::new(), raw: Vec::new(), left: Some(Box::new(edit.condition.clone())), right: None };
        assert!(document.set_trigger(policy, trigger, &bad).unwrap_err().contains("right part"));
        bad.condition = ConditionEdit { kind: C_GREAT, values: HashMap::new(), raw: Vec::new(), left: Some(Box::new(edit.condition.clone())), right: Some(Box::new(edit.condition.clone())) };
        assert!(document.set_trigger(policy, trigger, &bad).unwrap_err().contains("must be a number"));
        let mut old = document.file.policies[policy].triggers[trigger].clone();
        old.version = 11;
        let mut whisper = edit.clone();
        whisper.operations[talk].kind = O_WHISPER;
        assert!(build_trigger(&old, &whisper, false).unwrap_err().contains("2013 server"));
        assert!(build_trigger(&old, &whisper, true).is_ok(), "the newer server reads newer types in version-11 triggers");

        // Clone a trigger and a policy: next free IDs; delete; one undo step each.
        let triggers = document.file.policies[policy].triggers.len();
        let max_trigger = document.file.policies[policy].triggers.iter().map(|entry| entry.id).max().unwrap();
        document.trigger_action(policy, trigger, &ListAction::Clone).unwrap();
        assert_eq!(document.file.policies[policy].triggers[trigger + 1].id, max_trigger + 1);
        let policies = document.file.policies.len();
        let max_policy = document.file.policies.iter().map(|entry| entry.id).max().unwrap();
        let result = document.policy_action(policy, &ListAction::Clone).unwrap();
        assert_eq!(result.policy.unwrap().id, max_policy + 1);
        assert_eq!(document.file.policies.len(), policies + 1);
        document.undo().unwrap();
        assert_eq!(document.file.policies.len(), policies);
        document.undo().unwrap();
        assert_eq!(document.file.policies[policy].triggers.len(), triggers);

        // Save, reopen: the edit is there, everything else as before.
        let report = document.save(None, true, false).unwrap();
        assert!(report.backup.is_some());
        assert!(!document.edit_state().dirty);
        let reopened = Document::open(path.to_str().unwrap()).unwrap();
        assert_eq!(reopened.file, document.file);
        let original = Document::open(&source).unwrap();
        let differing = original.file.policies.iter().zip(&reopened.file.policies).filter(|(a, b)| a != b).count();
        assert_eq!(differing, 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn problems_of_official_files() {
        for path in samples() {
            let document = Document::open(&path).unwrap();
            let problems = document.problems(&ProblemContext::default());
            let count = |severity: &str| problems.iter().filter(|problem| problem.severity == severity).count();
            println!("PROBLEMS {path}: {} errors, {} warnings, {} notes", count("error"), count("warning"), count("note"));
            for problem in problems.iter().filter(|problem| problem.severity != "note").take(6) {
                println!("PROBLEMS   {} {:?}", problem.severity, problem.message);
            }
            // Every sample repeats one policy ID; official files load, so nothing is an error.
            assert!(problems.iter().any(|problem| problem.message.contains("server keeps the first")));
            assert_eq!(count("error"), 0, "{path}");
        }
    }

    #[test]
    fn cleanup_removes_subtriggers_nothing_runs() {
        for path in samples() {
            let mut document = Document::open(&path).unwrap();
            let unused = document.unused_subtriggers(None);
            let total: usize = unused.iter().map(|(_, ids)| ids.len()).sum();
            let triggers: usize = document.file.policies.iter().map(|policy| policy.triggers.len()).sum();
            println!("CLEANUP {path}: {total} unused sub-triggers in {} policies", unused.len());
            let original = document.file.clone();
            document.cleanup(None, None).unwrap();
            assert!(document.unused_subtriggers(None).is_empty());
            assert_eq!(document.file.policies.iter().map(|policy| policy.triggers.len()).sum::<usize>(), triggers - total);
            // Every remaining "run a trigger" still finds its trigger when it did before.
            for (before, after) in original.policies.iter().zip(&document.file.policies) {
                for trigger in after.triggers.iter().filter(|trigger| !trigger.run) {
                    for operation in trigger.operations.iter().filter(|operation| operation.kind == O_RUN_TRIGGER) {
                        let id = u32_at(&operation.param, 0) as u32;
                        assert_eq!(before.triggers.iter().any(|entry| entry.id == id), after.triggers.iter().any(|entry| entry.id == id));
                    }
                }
            }
            if total > 0 {
                document.undo().unwrap();
                assert_eq!(document.file, original);
            }
        }
    }

    #[test]
    fn views_name_categories_and_calls() {
        let Some(path) = samples().into_iter().nth(1) else { return };
        let document = Document::open(&path).unwrap();
        let view = document.view();
        assert_eq!(view.policies.len(), document.file.policies.len());
        assert_eq!(view.policies.iter().filter(|policy| policy.shadowed).count(), 1, "one repeated policy ID in every sample");
        // Every trigger renders, with a known category.
        let mut categories: HashMap<&str, usize> = HashMap::new();
        for index in 0..document.file.policies.len() {
            for trigger in document.policy(index).unwrap().triggers {
                *categories.entry(trigger.category).or_default() += 1;
                assert!(!trigger.expression.contains("unknown"), "{}", trigger.expression);
                assert!(trigger.operations.iter().all(|operation| operation.name != "Unknown operation"));
            }
        }
        assert!(categories.contains_key("Timer") && categories.contains_key("Birth") && categories.contains_key("Heartbeat"), "{categories:?}");
        // Sub-triggers are called by others.
        let with_calls = (0..document.file.policies.len()).filter_map(|index| document.policy(index).ok()).flat_map(|policy| policy.triggers).filter(|trigger| trigger.run).filter(|trigger| !trigger.called_by.is_empty()).count();
        assert!(with_calls > 100);
        // Search by a policy ID and by talk text.
        let first = document.file.policies[10].id;
        assert!(document.search(&first.to_string(), &HashMap::new()).contains(&10));
        assert!(!document.search("$B", &HashMap::new()).is_empty());
    }
}

