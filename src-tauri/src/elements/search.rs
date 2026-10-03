//! Advanced search over every record of the file.
//!
//! - **Value**: a number, text or byte pattern in any field (optionally in
//!   bytes no layout describes).
//! - **Conditions**: records whose named fields meet conditions (`=`, `<`,
//!   `in`, `contains`, `has flags`, …), all or any of them.
//!
//! Lists are flattened into leaf "slots" (struct members and array elements
//! included) and values are read straight from the record bytes, so whole
//! records are never decoded.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::decode::read_wstr;
use super::format::{EnumSet, Field, ListDef, Ty, TypeRule};

/// Results kept by default (matches past it are only counted).
pub const LIMIT: usize = 500;
const MATCHES_PER_RECORD: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueKind {
    Int,
    Float,
    Text,
    Hex,
}

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
    /// Negative conditions must hold for every slot of an array field;
    /// positive ones for any.
    fn every(self) -> bool {
        matches!(self, Op::Ne | Op::NotIn | Op::LacksFlags | Op::Empty)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Condition {
    pub field: String,
    pub op: Op,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum Query {
    #[serde(rename_all = "camelCase")]
    Value {
        value: String,
        kind: ValueKind,
        list: Option<usize>,
        #[serde(default)]
        include_unknown: bool,
        #[serde(default)]
        case_sensitive: bool,
    },
    #[serde(rename_all = "camelCase")]
    Conditions {
        conditions: Vec<Condition>,
        match_all: bool,
        list: Option<usize>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Match {
    /// Field path, e.g. "addons[2].id", or "+0x01F4" for undescribed bytes.
    pub field: String,
    pub off: usize,
    pub value: String,
    /// Enum or mask label of the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub list: usize,
    pub row: usize,
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    pub matches: Vec<Match>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub hits: Vec<Hit>,
    pub matched_records: usize,
    pub matched_lists: usize,
    pub scanned_lists: usize,
    pub scanned_records: usize,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

/// A field name of the open file, for suggestions.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldName {
    pub name: String,
    /// Lists that have it.
    pub lists: usize,
    /// "int", "float", "text" or "bytes".
    pub kind: &'static str,
    /// Enum or mask of the field, when one names its values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
}

// ---------------------------------------------------------------- slots

/// A conditional type, resolved against the controlling sibling's bytes.
#[derive(Debug, Clone)]
struct Rule {
    off: usize,
    ty: Ty,
    values: Vec<i64>,
    not: bool,
    t: Ty,
}

/// One scalar value of a record: a field, a struct member or an array element.
#[derive(Debug, Clone)]
pub struct Slot {
    /// "addons[2].id"
    pub path: String,
    /// "addons.id" (lowercase, no indexes), for dotted condition fields.
    plain: String,
    /// "id" (lowercase)
    leaf: String,
    pub off: usize,
    ty: Ty,
    rules: Vec<Rule>,
    pub set: Option<String>,
    /// Display role ("path", "time", …).
    pub display: Option<String>,
}

impl Slot {
    /// The slot's type in this record (conditional types applied).
    pub(crate) fn ty(&self, bytes: &[u8]) -> &Ty {
        for r in &self.rules {
            if let Some(Val::Int(v)) = read(&r.ty, bytes, r.off) {
                if r.values.contains(&v) != r.not {
                    return &r.t;
                }
            }
        }
        &self.ty
    }

    fn read(&self, bytes: &[u8]) -> Option<Val> {
        read(self.ty(bytes), bytes, self.off)
    }

    /// The value as JSON: numbers stay numbers.
    pub(crate) fn json(&self, bytes: &[u8]) -> serde_json::Value {
        match self.read(bytes) {
            Some(Val::Int(v)) => v.into(),
            Some(Val::Float(v)) => serde_json::Number::from_f64(v).map_or_else(|| v.to_string().into(), serde_json::Value::Number),
            Some(v) => v.show().into(),
            None => serde_json::Value::Null,
        }
    }

    /// The value as text (numbers as written in the file, text as is).
    pub(crate) fn text(&self, bytes: &[u8]) -> String {
        self.read(bytes).map(|v| v.show()).unwrap_or_default()
    }

    /// The field name (lowercase; an array's for its elements).
    pub(crate) fn name(&self) -> &str {
        &self.leaf
    }

    /// Bytes the slot takes (with its base type).
    pub(crate) fn size(&self) -> usize {
        self.ty.size()
    }

    /// The value, when the slot holds an integer in this record.
    pub(crate) fn int(&self, bytes: &[u8]) -> Option<i64> {
        match self.read(bytes)? {
            Val::Int(v) => Some(v),
            _ => None,
        }
    }

    fn names(&self, query: &str) -> bool {
        if query.contains('.') {
            self.plain == query
        } else {
            self.leaf == query
        }
    }
}

/// Every scalar slot of a list definition that fits in `size` bytes.
pub fn slots(def: &ListDef, size: usize) -> Vec<Slot> {
    let mut out = Vec::new();
    walk(&def.fields, 0, "", "", size, &mut out);
    out
}

fn walk(fields: &[Field], base: usize, path: &str, plain: &str, size: usize, out: &mut Vec<Slot>) {
    for f in fields {
        let path = if path.is_empty() { f.name.clone() } else { format!("{path}.{}", f.name) };
        let plain = if plain.is_empty() { f.name.to_lowercase() } else { format!("{plain}.{}", f.name.to_lowercase()) };
        let rules = f
            .when
            .iter()
            .filter_map(|r: &TypeRule| {
                let s = fields.iter().find(|s| s.name == r.field)?;
                Some(Rule { off: base + s.off, ty: s.t.clone(), values: r.values.clone(), not: r.not, t: r.t.clone() })
            })
            .collect();
        element(&f.t, base + f.off, &path, &plain, f, rules, size, out);
    }
}

#[allow(clippy::too_many_arguments)]
fn element(ty: &Ty, off: usize, path: &str, plain: &str, f: &Field, rules: Vec<Rule>, size: usize, out: &mut Vec<Slot>) {
    if off + ty.size() > size {
        return;
    }
    match ty {
        Ty::Struct { fields } => walk(fields, off, path, plain, size, out),
        Ty::Array { n, stride, t } => {
            for i in 0..*n {
                element(t, off + i * stride, &format!("{path}[{i}]"), plain, f, vec![], size, out);
            }
        }
        _ => out.push(Slot {
            path: path.into(),
            plain: plain.into(),
            leaf: f.name.to_lowercase(),
            off,
            ty: ty.clone(),
            rules,
            set: f.e.clone(),
            display: f.display.clone(),
        }),
    }
}

// ---------------------------------------------------------------- values

#[derive(Debug, Clone, PartialEq)]
enum Val {
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl Val {
    fn show(&self) -> String {
        match self {
            Val::Int(v) => v.to_string(),
            Val::Float(v) => v.to_string(),
            Val::Text(s) => s.clone(),
            Val::Bytes(b) => b.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
        }
    }

    fn number(&self) -> Option<f64> {
        match self {
            Val::Int(v) => Some(*v as f64),
            Val::Float(v) => Some(*v),
            _ => None,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Val::Int(v) => *v == 0,
            Val::Float(v) => *v == 0.0,
            Val::Text(s) => s.trim().is_empty(),
            Val::Bytes(b) => b.iter().all(|&b| b == 0),
        }
    }
}

fn read(ty: &Ty, b: &[u8], off: usize) -> Option<Val> {
    let get = |n: usize| b.get(off..off + n);
    Some(match ty {
        Ty::I8 => Val::Int(*b.get(off)? as i8 as i64),
        Ty::U8 | Ty::Bool => Val::Int(*b.get(off)? as i64),
        Ty::I16 => Val::Int(i16::from_le_bytes(get(2)?.try_into().ok()?) as i64),
        Ty::U16 => Val::Int(u16::from_le_bytes(get(2)?.try_into().ok()?) as i64),
        Ty::I32 => Val::Int(i32::from_le_bytes(get(4)?.try_into().ok()?) as i64),
        Ty::U32 => Val::Int(u32::from_le_bytes(get(4)?.try_into().ok()?) as i64),
        Ty::I64 | Ty::U64 => Val::Int(i64::from_le_bytes(get(8)?.try_into().ok()?)),
        Ty::F32 => Val::Float(f32::from_le_bytes(get(4)?.try_into().ok()?) as f64),
        Ty::F64 => Val::Float(f64::from_le_bytes(get(8)?.try_into().ok()?)),
        Ty::Wstr { n } => Val::Text(read_wstr(get(n * 2)?)),
        Ty::Str { n } => {
            let raw = get(*n)?;
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            Val::Text(encoding_rs::GBK.decode(&raw[..end]).0.into_owned())
        }
        Ty::Bytes { n } => Val::Bytes(get(*n)?.to_vec()),
        Ty::Array { .. } | Ty::Struct { .. } => return None,
    })
}

fn kind_of(ty: &Ty) -> &'static str {
    match ty {
        Ty::F32 | Ty::F64 => "float",
        Ty::Wstr { .. } | Ty::Str { .. } => "text",
        Ty::Bytes { .. } => "bytes",
        _ => "int",
    }
}

/// "1291", "-5", "0x50B".
pub fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    s.parse().ok()
}

/// A number, or a label of the field's enum (or one bit of its mask).
fn parse_value(s: &str, set: Option<&EnumSet>) -> Option<f64> {
    let s = s.trim();
    if let Some(v) = parse_int(s) {
        return Some(v as f64);
    }
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    let set = set?;
    set.items.iter().find(|(_, label)| label.eq_ignore_ascii_case(s)).and_then(|(k, _)| k.parse::<i64>().ok()).map(|v| v as f64)
}

/// Bits: a number, or mask labels joined by "," or "|".
fn parse_bits(s: &str, set: Option<&EnumSet>) -> Option<u64> {
    if let Some(v) = parse_int(s) {
        return Some(v as u64);
    }
    s.split([',', '|']).map(str::trim).filter(|p| !p.is_empty()).try_fold(0u64, |acc, part| Some(acc | parse_value(part, set)? as u64))
}

// ---------------------------------------------------------------- conditions

/// A condition bound to one list's slots, its value parsed for them.
struct Bound<'a> {
    op: Op,
    text: String,
    slots: Vec<&'a Slot>,
    number: Option<f64>,
    numbers: Vec<f64>,
    bits: Option<u64>,
}

fn bind<'a, 's>(c: &Condition, slots: &'a [Slot], set_of: &dyn Fn(&str) -> Option<&'s EnumSet>) -> Result<Option<Bound<'a>>, String> {
    let name = c.field.trim().to_lowercase();
    let matching: Vec<&Slot> = slots.iter().filter(|s| s.names(&name)).collect();
    let Some(first) = matching.first() else { return Ok(None) };
    let set = first.set.as_deref().and_then(set_of);
    let value = c.value.trim();
    let mut b = Bound { op: c.op, text: value.to_lowercase(), slots: matching.clone(), number: None, numbers: vec![], bits: None };
    match c.op {
        Op::Eq | Op::Ne => b.number = parse_value(value, set),
        Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            b.number = Some(parse_value(value, set).ok_or_else(|| format!("Enter a number to compare “{}” with.", c.field))?);
        }
        Op::In | Op::NotIn => {
            b.numbers = value
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|p| parse_value(p, set).ok_or_else(|| format!("“{p}” is not a number or label of “{}”.", c.field)))
                .collect::<Result<_, _>>()?;
            if b.numbers.is_empty() {
                return Err(format!("List the values for “{}”, e.g. 7, 8, 15.", c.field));
            }
        }
        Op::HasFlags | Op::LacksFlags => {
            b.bits = Some(parse_bits(value, set).ok_or_else(|| format!("Enter the flags of “{}” as a number or mask labels.", c.field))?);
        }
        Op::Contains | Op::Starts | Op::Ends | Op::Empty | Op::NotEmpty => {}
    }
    Ok(Some(b))
}

fn meets(b: &Bound, v: &Val) -> bool {
    let num = v.number();
    let text = || v.show().to_lowercase();
    match b.op {
        Op::Eq => match (num, b.number) {
            (Some(x), Some(y)) => x == y,
            _ => text() == b.text,
        },
        Op::Ne => match (num, b.number) {
            (Some(x), Some(y)) => x != y,
            _ => text() != b.text,
        },
        Op::Lt => matches!((num, b.number), (Some(x), Some(y)) if x < y),
        Op::Le => matches!((num, b.number), (Some(x), Some(y)) if x <= y),
        Op::Gt => matches!((num, b.number), (Some(x), Some(y)) if x > y),
        Op::Ge => matches!((num, b.number), (Some(x), Some(y)) if x >= y),
        Op::In => num.is_some_and(|x| b.numbers.contains(&x)),
        Op::NotIn => num.is_some_and(|x| !b.numbers.contains(&x)),
        Op::Contains => text().contains(&b.text),
        Op::Starts => text().starts_with(&b.text),
        Op::Ends => text().ends_with(&b.text),
        Op::HasFlags => matches!((v, b.bits), (Val::Int(x), Some(m)) if (*x as u64) & m == m),
        Op::LacksFlags => matches!((v, b.bits), (Val::Int(x), Some(m)) if (*x as u64) & m == 0),
        Op::Empty => v.is_empty(),
        Op::NotEmpty => !v.is_empty(),
    }
}

/// Slots of a record meeting a bound condition (empty when it fails).
fn check<'a>(b: &Bound<'a>, bytes: &[u8]) -> Vec<(&'a Slot, Val)> {
    let mut hits = Vec::new();
    for slot in &b.slots {
        let Some(v) = slot.read(bytes) else { continue };
        if meets(b, &v) {
            hits.push((*slot, v));
        } else if b.op.every() {
            return vec![];
        }
    }
    hits
}

// ---------------------------------------------------------------- the search

/// What the search needs from the open document, per list.
pub struct ListView<'a> {
    pub index: usize,
    pub item_size: usize,
    pub count: usize,
    /// The file and where the list's records start in it.
    pub data: &'a [u8],
    pub data_offset: usize,
    pub slots: Vec<Slot>,
}

impl ListView<'_> {
    fn record(&self, row: usize) -> &[u8] {
        let at = self.data_offset + row * self.item_size;
        &self.data[at..at + self.item_size]
    }
}

pub struct Searcher<'a> {
    pub set_of: &'a dyn Fn(&str) -> Option<&'a EnumSet>,
    /// (id, name, icon) of a record.
    pub describe: &'a dyn Fn(usize, &[u8]) -> (u32, String, Option<u32>),
    /// Results kept (usually [`LIMIT`]; exports keep all).
    pub limit: usize,
}

impl Searcher<'_> {
    pub fn run(&self, query: &Query, lists: &[ListView]) -> Result<Report, String> {
        let started = Instant::now();
        let mut report = Report::default();
        let scope = match query {
            Query::Value { list, .. } | Query::Conditions { list, .. } => *list,
        };
        let lists = lists.iter().filter(|l| scope.map_or(true, |s| s == l.index));
        match query {
            Query::Conditions { conditions, match_all, .. } => {
                if conditions.is_empty() {
                    return Err("Add a condition.".into());
                }
                for c in conditions {
                    if c.field.trim().is_empty() {
                        return Err("Pick a field for every condition.".into());
                    }
                }
                for list in lists {
                    self.conditions(conditions, *match_all, list, &mut report)?;
                }
            }
            Query::Value { value, kind, include_unknown, case_sensitive, .. } => {
                let needle = Needle::parse(value, *kind, *case_sensitive)?;
                for list in lists {
                    self.value(&needle, *include_unknown, list, &mut report);
                }
            }
        }
        report.truncated = report.matched_records > report.hits.len();
        report.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(report)
    }

    fn conditions(&self, conditions: &[Condition], all: bool, list: &ListView, report: &mut Report) -> Result<(), String> {
        let mut bound = Vec::new();
        for c in conditions {
            match bind(c, &list.slots, self.set_of)? {
                Some(b) => bound.push(b),
                None if all => return Ok(()),
                None => {}
            }
        }
        if bound.is_empty() {
            return Ok(());
        }
        report.scanned_lists += 1;
        report.scanned_records += list.count;
        let mut matched_here = false;
        'rows: for row in 0..list.count {
            let bytes = list.record(row);
            let mut found = Vec::new();
            for b in &bound {
                let hits = check(b, bytes);
                if hits.is_empty() {
                    if all {
                        continue 'rows;
                    }
                } else {
                    found.extend(hits);
                }
            }
            if found.is_empty() {
                continue;
            }
            matched_here = true;
            self.add(report, list.index, row, bytes, found.into_iter().map(|(s, v)| self.matched(s, &v)).collect());
        }
        report.matched_lists += matched_here as usize;
        Ok(())
    }

    fn value(&self, needle: &Needle, include_unknown: bool, list: &ListView, report: &mut Report) {
        let described = !list.slots.is_empty();
        if !described && !include_unknown && needle.kind != ValueKind::Hex {
            return;
        }
        report.scanned_lists += 1;
        report.scanned_records += list.count;
        // Bytes the layout covers, for telling unknown hits apart.
        let mut covered = vec![false; list.item_size];
        for s in &list.slots {
            let end = (s.off + s.ty.size()).min(list.item_size);
            covered[s.off..end].iter_mut().for_each(|c| *c = true);
        }
        let mut matched_here = false;
        for row in 0..list.count {
            let bytes = list.record(row);
            let mut found: Vec<Match> = Vec::new();
            match needle.kind {
                ValueKind::Hex => {
                    let pattern = &needle.bytes;
                    let mut at = 0;
                    while let Some(p) = find(&bytes[at..], pattern) {
                        let off = at + p;
                        let slot = list.slots.iter().find(|s| off >= s.off && off < s.off + s.ty(bytes).size());
                        found.push(match slot {
                            Some(s) => Match { field: s.path.clone(), off: s.off, value: s.read(bytes).map(|v| v.show()).unwrap_or_default(), label: None },
                            None => unknown(off, needle),
                        });
                        at = off + 1;
                        if found.len() >= MATCHES_PER_RECORD {
                            break;
                        }
                    }
                }
                _ => {
                    for s in &list.slots {
                        if let Some(v) = s.read(bytes) {
                            if needle.matches(&v) {
                                found.push(self.matched(s, &v));
                            }
                        }
                    }
                    if include_unknown && matches!(needle.kind, ValueKind::Int | ValueKind::Float) {
                        let mut off = 0;
                        while off + 4 <= bytes.len() {
                            if !covered.get(off..off + 4).is_some_and(|c| c.iter().any(|&c| c)) && bytes[off..off + 4] == needle.bytes[..] {
                                found.push(unknown(off, needle));
                            }
                            off += 4;
                        }
                    }
                }
            }
            if !found.is_empty() {
                matched_here = true;
                self.add(report, list.index, row, bytes, found);
            }
        }
        report.matched_lists += matched_here as usize;
    }

    fn matched(&self, s: &Slot, v: &Val) -> Match {
        let label = match v {
            Val::Int(x) => s.set.as_deref().and_then(self.set_of).and_then(|set| set.label_for(*x)),
            _ => None,
        };
        Match { field: s.path.clone(), off: s.off, value: v.show(), label }
    }

    fn add(&self, report: &mut Report, list: usize, row: usize, bytes: &[u8], mut matches: Vec<Match>) {
        report.matched_records += 1;
        if report.hits.len() >= self.limit {
            return;
        }
        matches.truncate(MATCHES_PER_RECORD);
        let (id, name, icon) = (self.describe)(list, bytes);
        report.hits.push(Hit { list, row, id, name, icon, matches });
    }
}

fn unknown(off: usize, needle: &Needle) -> Match {
    Match { field: format!("+0x{off:04X}"), off, value: needle.shown.clone(), label: None }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

struct Needle {
    kind: ValueKind,
    int: i64,
    float: f64,
    text: String,
    case_sensitive: bool,
    /// The value's bytes (int32 / float32 / hex pattern).
    bytes: Vec<u8>,
    shown: String,
}

impl Needle {
    fn parse(value: &str, kind: ValueKind, case_sensitive: bool) -> Result<Self, String> {
        let value = value.trim();
        let mut n = Needle { kind, int: 0, float: 0.0, text: String::new(), case_sensitive, bytes: vec![], shown: value.into() };
        match kind {
            ValueKind::Int => {
                n.int = parse_int(value).ok_or("Enter a whole number, e.g. 1291 or 0x50B.")?;
                n.bytes = (n.int as u32).to_le_bytes().to_vec();
            }
            ValueKind::Float => {
                n.float = value.parse().map_err(|_| "Enter a number, e.g. 1.5.")?;
                n.bytes = (n.float as f32).to_le_bytes().to_vec();
            }
            ValueKind::Text => {
                if value.is_empty() {
                    return Err("Enter some text to search for.".into());
                }
                n.text = if case_sensitive { value.into() } else { value.to_lowercase() };
            }
            ValueKind::Hex => {
                let hex: String = value.replace("0x", "").replace("0X", "").chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
                if hex.is_empty() || hex.len() % 2 != 0 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err("Enter hex bytes such as “0B 05 00 00”.".into());
                }
                n.bytes = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
                n.shown = n.bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
            }
        }
        Ok(n)
    }

    fn matches(&self, v: &Val) -> bool {
        match (self.kind, v) {
            (ValueKind::Int, Val::Int(x)) => *x == self.int || (*x as u32 as i64) == self.int,
            (ValueKind::Float, Val::Float(x)) => (*x as f32) == (self.float as f32),
            (ValueKind::Text, Val::Text(s)) => {
                if self.case_sensitive {
                    s.contains(&self.text)
                } else {
                    s.to_lowercase().contains(&self.text)
                }
            }
            _ => false,
        }
    }
}

/// Field names of the given definitions, with how many lists have each.
pub fn field_names<'a>(defs: impl Iterator<Item = (&'a ListDef, usize)>) -> Vec<FieldName> {
    let mut names: HashMap<String, FieldName> = HashMap::new();
    for (def, size) in defs {
        let mut seen = std::collections::HashSet::new();
        for s in slots(def, size) {
            for name in [s.leaf.clone(), s.plain.clone()] {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let e = names.entry(name.clone()).or_insert(FieldName { name, lists: 0, kind: kind_of(&s.ty), set: s.set.clone() });
                e.lists += 1;
            }
        }
    }
    let mut out: Vec<FieldName> = names.into_values().collect();
    out.sort_by(|a, b| b.lists.cmp(&a.lists).then_with(|| a.name.cmp(&b.name)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_labels_and_bits() {
        assert_eq!(parse_int("0x50B"), Some(1291));
        assert_eq!(parse_int("-5"), Some(-5));
        let mut items = HashMap::new();
        items.insert("1".to_string(), "Cannot be dropped".to_string());
        items.insert("16".to_string(), "Cannot be traded".to_string());
        let set = EnumSet { label: String::new(), flags: true, items, descriptions: HashMap::new() };
        assert_eq!(parse_value("cannot be traded", Some(&set)), Some(16.0));
        assert_eq!(parse_bits("Cannot be dropped | Cannot be traded", Some(&set)), Some(17));
        assert_eq!(parse_bits("0x11", None), Some(17));
    }
}
