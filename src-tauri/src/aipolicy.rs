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
use serde::Serialize;

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

// Writing is proven by the round-trip test; the browser is read-only until editing arrives.
#[allow(dead_code)]
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
#[allow(dead_code)]
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

/// A parameter as the browser shows it. `kind` says what an ID refers to, so the UI can name it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub label: &'static str,
    pub value: ParamValue,
    /// `skill`, `monster`, `item`, `mine`, `task`, `trigger`, `timer`, `path`, `controller`, `global`, `event`.
    pub refers: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ParamValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
}

fn int(label: &'static str, value: i64, refers: Option<&'static str>) -> Param {
    Param { label, value: ParamValue::Int(value), refers }
}

fn float(label: &'static str, value: f64) -> Param {
    Param { label, value: ParamValue::Float((value * 1_000_000.0).round() / 1_000_000.0), refers: None }
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

fn condition_params(node: &Condition) -> Vec<Param> {
    let p = &node.param;
    match node.kind {
        C_TIME_COME => vec![int("timer", u32_at(p, 0), Some("timer"))],
        C_HP_LESS => vec![float("ratio", f32_at(p, 0))],
        C_RANDOM => vec![float("chance", f32_at(p, 0))],
        C_PATH_END => vec![int("path", u32_at(p, 0), Some("path"))],
        C_ENMITY_REACH => vec![int("players", i32_at(p, 0), None), int("player level (unused)", i32_at(p, 4), None)],
        C_DISTANCE_REACH => vec![float("distance", f32_at(p, 0))],
        C_ATTACK_BY_SKILL => vec![int("skill", u32_at(p, 0), Some("skill"))],
        C_VAR => vec![int("variable", i32_at(p, 0), Some("global"))],
        C_CONSTANT => vec![int("value", i32_at(p, 0), None)],
        C_RANK_LEVEL => vec![int("place", i32_at(p, 0), None)],
        _ => Vec::new(),
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

fn operation_params(operation: &Operation) -> Vec<Param> {
    let p = &operation.param;
    match operation.kind {
        0 => vec![int("type", u32_at(p, 0), Some("attack"))],
        1 => vec![int("skill", u32_at(p, 0), Some("skill")), int("level", u32_at(p, 4), None)],
        O_TALK | O_WHISPER | O_TALK_PORTRAIT => vec![Param { label: "text", value: ParamValue::Text(utf16(p)), refers: None }],
        O_RUN_TRIGGER | O_STOP_TRIGGER | O_ACTIVE_TRIGGER => vec![int("trigger", u32_at(p, 0), Some("trigger"))],
        O_CREATE_TIMER => vec![int("timer", u32_at(p, 0), Some("timer")), int("period (s)", u32_at(p, 4), None), int("times (0 = forever)", u32_at(p, 8), None)],
        O_KILL_TIMER => vec![int("timer", u32_at(p, 0), Some("timer"))],
        O_ACTIVE_CONTROLLER => {
            let mut out = vec![int("controller", u32_at(p, 0), Some("controller"))];
            if p.len() >= 5 {
                out.push(Param { label: "stop", value: ParamValue::Bool(p[4] != 0), refers: None });
            }
            out
        }
        15 => vec![
            int("monster", u32_at(p, 0), Some("monster")),
            int("looks like", u32_at(p, 4), Some("monster")),
            int("count", u32_at(p, 8), None),
            int("lifetime (s, 0 = until killed)", u32_at(p, 12), None),
            Param { label: "name", value: ParamValue::Text(utf16(p.get(16..48).unwrap_or(&[]))), refers: None },
            float("range (m)", f32_at(p, 48)),
            Param { label: "follows", value: ParamValue::Bool(p.get(52).is_some_and(|&b| b != 0)), refers: None },
            Param { label: "disappears with it", value: ParamValue::Bool(p.get(53).is_some_and(|&b| b != 0)), refers: None },
        ],
        16 => vec![int("task", u32_at(p, 0), Some("task"))],
        O_CHANGE_PATH => {
            let mut out = vec![int("path", u32_at(p, 0), Some("path"))];
            if p.len() >= 8 {
                out.push(int("type", i32_at(p, 4), Some("path_type")));
            }
            out
        }
        19 => vec![float("range (m)", f32_at(p, 0))],
        20 => vec![int("skill", u32_at(p, 0), Some("skill")), int("level", u32_at(p, 4), None), float("range (m)", f32_at(p, 8))],
        22 => vec![int("variable", i32_at(p, 0), Some("global")), int("value", i32_at(p, 4), None)],
        23 => vec![int("variable", i32_at(p, 0), Some("global")), int("add", i32_at(p, 4), None)],
        24 => vec![int("from", i32_at(p, 0), Some("global")), int("to", i32_at(p, 4), Some("global"))],
        25 => vec![
            int("mine", u32_at(p, 0), Some("mine")),
            int("count", u32_at(p, 4), None),
            int("HP", u32_at(p, 8), None),
            float("range (m)", f32_at(p, 12)),
            Param { label: "bound to the target", value: ParamValue::Bool(p.get(16).is_some_and(|&b| b != 0)), refers: None },
        ],
        26 | 30 => vec![int("item", u32_at(p, 0), Some("item")), int("count", u32_at(p, 4), None), int("expires (s, 0 = never)", u32_at(p, 8), None)],
        27 => vec![int("aggro", i32_at(p, 0), None)],
        28 | 29 => vec![int("event", i32_at(p, 0), Some("event"))],
        _ => Vec::new(),
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
        C_ENMITY_REACH => format!("≥ {} players in aggro", i32_at(p, 0)),
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
    pub left: Option<Box<ConditionView>>,
    pub right: Option<Box<ConditionView>>,
}

fn condition_view(node: &Condition) -> ConditionView {
    ConditionView {
        kind: node.kind,
        name: condition_name(node.kind),
        params: condition_params(node),
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
}

pub struct Document {
    pub path: String,
    pub size: usize,
    pub file: PolicyFile,
}

impl Document {
    /// The first policy of an ID (the one the server uses) and its trigger count.
    pub fn policy_by_id(&self, id: u32) -> Option<(usize, usize)> {
        self.file.policies.iter().position(|policy| policy.id == id).map(|index| (index, self.file.policies[index].triggers.len()))
    }

    pub fn open(path: &str) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|error| format!("Could not read {path}: {error}"))?;
        let file = parse(&data).map_err(|error| format!("{path}: {error}"))?;
        Ok(Document { path: path.to_string(), size: data.len(), file })
    }

    fn first_of_ids(&self) -> HashMap<u32, usize> {
        let mut out = HashMap::new();
        for (index, policy) in self.file.policies.iter().enumerate() {
            out.entry(policy.id).or_insert(index);
        }
        out
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
                }
            })
            .collect();
        let mut versions: Vec<(u32, usize)> = versions.into_iter().collect();
        versions.sort_unstable();
        FileView { path: self.path.clone(), size: self.size, policies, triggers: self.file.policies.iter().map(|policy| policy.triggers.len()).sum(), versions }
    }

    pub fn policy(&self, index: usize) -> Result<PolicyView, String> {
        let policy = self.file.policies.get(index).ok_or_else(|| format!("No policy {}", index + 1))?;
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
            .map(|(index, trigger)| TriggerView {
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
                        params: operation_params(operation),
                        target: operation.target,
                        target_name: target_name(operation.target),
                        uses_target: needs_target(operation.kind),
                        target_mask: operation.target_param,
                    })
                    .collect(),
                called_by: called_by.get(&trigger.id).cloned().unwrap_or_default(),
            })
            .collect();
        let shadowed_by = self.first_of_ids().get(&policy.id).copied().filter(|&first| first != index);
        Ok(PolicyView { index, id: policy.id, triggers, shadowed_by })
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
                            operation_params(operation).iter().any(|param| match &param.value {
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


