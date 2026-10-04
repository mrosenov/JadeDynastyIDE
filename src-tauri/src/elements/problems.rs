//! The Problems panel: things in the file that the client may trip over, or
//! that look wrong.
//!
//! Errors: duplicate IDs within a list, references to records or dialogs
//! that do not exist. Warnings: IDs one list's record hides from another's
//! (same ID space), dialog options to missing windows,
//! client paths missing from path.data, texts without a terminator, values
//! an enum does not name. Info: mask bits nobody named, lists whose layout
//! does not fit exactly.

use std::collections::HashMap;
use std::time::Instant;

use serde::Serialize;

use super::format::{EnumSet, Ty};
use super::refs::{registry_space, IdSpace};
use super::{search, Document, LayoutFit};

/// Problems kept per kind (the rest are only counted).
const PER_KIND: usize = 1000;

/// The value to group into one enum/mask problem, or none when the set names it.
fn unnamed_set_value(set: &EnumSet, value: i64, all_bits: u64) -> Option<u64> {
    if !set.flags {
        return (!set.items.contains_key(&value.to_string())).then_some(value as u64);
    }
    let bits = (value as u64) & all_bits;
    if bits == all_bits {
        return None;
    }
    let unnamed = (0..64)
        .map(|bit| 1u64 << bit)
        .filter(|bit| bits & bit != 0 && !set.items.contains_key(&bit.to_string()))
        .fold(0, |found, bit| found | bit);
    (unnamed != 0).then_some(unnamed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    DuplicateId,
    ShadowedId,
    BrokenRef,
    MissingDialog,
    DialogWindow,
    MissingPath,
    UnterminatedText,
    UnknownEnum,
    UnnamedBits,
    Layout,
}

impl Kind {
    const ALL: [Kind; 10] = [
        Kind::DuplicateId,
        Kind::BrokenRef,
        Kind::MissingDialog,
        Kind::ShadowedId,
        Kind::DialogWindow,
        Kind::MissingPath,
        Kind::UnterminatedText,
        Kind::UnknownEnum,
        Kind::UnnamedBits,
        Kind::Layout,
    ];

    fn severity(self) -> Severity {
        match self {
            Kind::DuplicateId | Kind::BrokenRef | Kind::MissingDialog => Severity::Error,
            Kind::ShadowedId | Kind::DialogWindow | Kind::MissingPath | Kind::UnterminatedText | Kind::UnknownEnum => Severity::Warning,
            Kind::UnnamedBits | Kind::Layout => Severity::Info,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::DuplicateId => "Duplicate IDs in a list",
            Kind::ShadowedId => "IDs hidden by another list",
            Kind::BrokenRef => "Broken references",
            Kind::MissingDialog => "Missing dialogs",
            Kind::DialogWindow => "Dialog options to missing windows",
            Kind::MissingPath => "Paths missing from path.data",
            Kind::UnterminatedText => "Texts without a terminator",
            Kind::UnknownEnum => "Values their enum does not name",
            Kind::UnnamedBits => "Mask bits without a name",
            Kind::Layout => "Lists without an exact layout",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Kind::DuplicateId => "Records of one list sharing an ID. The client keeps one ID → record map per ID space, so only the later record can be found by ID.",
            Kind::ShadowedId => "Records of different lists in the same ID space (items, addons, recipes, configs) sharing an ID. The client, server and editor register them in one map, so the record loaded later hides the earlier one from ID lookups. The official data has a few of these (mostly old or test entries).",
            Kind::BrokenRef => "A field declared to point at a list holds an ID that list does not have.",
            Kind::MissingDialog => "A record's id_dialog names an NPC dialog the file does not have.",
            Kind::DialogWindow => "An NPC dialog option opens a window the dialog does not have.",
            Kind::MissingPath => "A path or icon field holds an ID the client's path.data does not know.",
            Kind::UnterminatedText => "A text fills its whole field with no terminating zero; the client may read past it.",
            Kind::UnknownEnum => "A field's value is not one of its enum's values (the enum may be incomplete).",
            Kind::UnnamedBits => "Bits are set that the field's mask does not name (the mask may be incomplete).",
            Kind::Layout => "Lists read with another version's layout, a partial one or none, so their fields may be off.",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub kind: Kind,
    /// The record (or, for dialogs, the dialog index as row and no list).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off: Option<usize>,
    pub message: String,
    /// For dialog problems: the dialog's index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talk: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindSummary {
    pub kind: Kind,
    pub severity: Severity,
    pub title: &'static str,
    pub description: &'static str,
    pub count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kinds: Vec<KindSummary>,
    pub problems: Vec<Problem>,
    /// Whether some kinds have more problems than listed.
    pub truncated: bool,
    /// Path checks need the client's path.data.
    pub paths_checked: bool,
    pub elapsed_ms: u64,
}

struct Collector {
    counts: HashMap<Kind, usize>,
    problems: Vec<Problem>,
}

impl Collector {
    fn add(&mut self, p: Problem) {
        let n = self.counts.entry(p.kind).or_default();
        *n += 1;
        if *n <= PER_KIND {
            self.problems.push(p);
        }
    }
}

impl Document {
    fn problem(&self, kind: Kind, list: usize, row: usize, field: Option<(&str, usize)>, message: String) -> Problem {
        let bytes = self.file.record(list, row).unwrap_or_default();
        let def = self.def(list).map(|(_, d)| d);
        Problem {
            kind,
            list: Some(list),
            row: Some(row),
            id: Self::record_id(bytes),
            name: Self::record_name(bytes, Self::name_field(def)),
            icon: self.record_icon(bytes, Self::icon_field(def)),
            field: field.map(|(f, _)| f.to_string()),
            off: field.map(|(_, o)| o),
            message,
            talk: None,
        }
    }

    /// "Equipment Essence › Sword" for messages.
    fn describe_record(&self, list: usize, row: usize) -> String {
        let bytes = self.file.record(list, row).unwrap_or_default();
        let name = Self::record_name(bytes, Self::name_field(self.def(list).map(|(_, d)| d)));
        format!("{} › {}", self.list_name(list), if name.is_empty() { format!("#{row}") } else { name })
    }

    pub fn problems(&self) -> Report {
        let started = Instant::now();
        let mut c = Collector { counts: HashMap::new(), problems: Vec::new() };
        self.check_duplicate_ids(&mut c);
        self.check_refs(&mut c);
        self.check_values(&mut c);
        self.check_dialogs(&mut c);
        self.check_layouts(&mut c);

        let truncated = c.counts.values().any(|&n| n > PER_KIND);
        c.problems.sort_by_key(|p| (p.kind.severity(), Kind::ALL.iter().position(|k| *k == p.kind), p.list, p.row));
        let kinds = Kind::ALL
            .iter()
            .map(|&kind| KindSummary { kind, severity: kind.severity(), title: kind.title(), description: kind.description(), count: c.counts.get(&kind).copied().unwrap_or(0) })
            .collect();
        let paths_checked = self.resources.as_ref().is_some_and(|r| r.paths().is_ok());
        Report { kinds, problems: c.problems, truncated, paths_checked, elapsed_ms: started.elapsed().as_millis() as u64 }
    }

    /// Records sharing an ID within the space the client registers them in.
    /// `elementdataman::add_id_index` overwrites the map entry, so the
    /// record loaded later (file order) wins.
    fn check_duplicate_ids(&self, c: &mut Collector) {
        let mut seen: HashMap<(IdSpace, u32), (usize, usize)> = HashMap::new();
        for list in 0..self.file.lists.len() {
            let Some(struct_name) = self.lists[list].struct_name.as_deref() else { continue };
            if struct_name.to_ascii_uppercase().starts_with("UNKNOWN") {
                continue;
            }
            let space = registry_space(struct_name);
            for row in 0..self.file.lists[list].count {
                let id = Self::record_id(self.file.record(list, row).unwrap());
                if id == 0 {
                    continue;
                }
                if let Some((l, r)) = seen.insert((space, id), (list, row)) {
                    let (kind, message) = if l == list {
                        (Kind::DuplicateId, format!("ID {id} is also used by row {r} of this list ({}); lookups by ID find this record", self.describe_record(l, r)))
                    } else {
                        (Kind::ShadowedId, format!("ID {id} is also used by {} (same ID space); lookups by ID find this record, not that one", self.describe_record(l, r)))
                    };
                    c.add(self.problem(kind, list, row, Some(("id", 0)), message));
                }
            }
        }
    }

    /// Fields whose `refs` name lists that do not have the value.
    fn check_refs(&self, c: &mut Collector) {
        for site in self.sites().iter().filter(|s| !s.refs.is_empty()) {
            let targets: Vec<usize> = site.refs.iter().filter_map(|s| self.by_struct.get(s)).flatten().copied().collect();
            // A target list this file does not have: nothing to check against.
            if targets.is_empty() {
                continue;
            }
            let block = &self.file.lists[site.list];
            if site.off + 4 > block.item_size {
                continue;
            }
            for row in 0..block.count {
                let bytes = self.file.record(site.list, row).unwrap();
                let value = i32::from_le_bytes(bytes[site.off..site.off + 4].try_into().unwrap());
                if value <= 0 || targets.iter().any(|&t| self.id_index(t).contains_key(&(value as u32))) {
                    continue;
                }
                let names: Vec<String> = targets.iter().map(|&t| self.list_name(t)).collect();
                let message = format!("{} = {value}, not found in {}", site.path, names.join(" or "));
                c.add(self.problem(Kind::BrokenRef, site.list, row, Some((&site.path, site.off)), message));
            }
        }
    }

    /// Per-slot checks: dialogs, paths, texts, enums and masks.
    fn check_values(&self, c: &mut Collector) {
        let talks = self.talk_data().ok();
        let paths = self.resources.as_ref().and_then(|r| r.paths().ok());
        for list in 0..self.file.lists.len() {
            let Some((layout, def)) = self.def(list) else { continue };
            let block = &self.file.lists[list];
            let slots = search::slots(def, block.item_size);
            for slot in &slots {
                let set = slot.set.as_deref().and_then(|k| self.catalog.enum_set(Some(layout), k));
                let is_dialog = slot.path.eq_ignore_ascii_case("id_dialog");
                let is_path = matches!(slot.display.as_deref(), Some("path" | "icon"));
                let is_text = matches!(slot.ty(&[]), Ty::Wstr { .. });
                if set.is_none() && !is_dialog && !is_path && !is_text {
                    continue;
                }
                // Enum and mask findings are reported once per field and
                // value: (first row, records).
                let mut odd: Vec<(u64, usize, usize)> = Vec::new();
                let width = slot.ty(&[]).size() * 8;
                let all_bits = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
                for row in 0..block.count {
                    let bytes = self.file.record(list, row).unwrap();
                    let at = Some((slot.path.as_str(), slot.off));
                    if let Ty::Wstr { n } = slot.ty(bytes) {
                        let raw = &bytes[slot.off..slot.off + n * 2];
                        if *n > 0 && raw.chunks_exact(2).all(|u| u != [0, 0]) {
                            c.add(self.problem(Kind::UnterminatedText, list, row, at, format!("{} fills all {n} characters with no terminator", slot.path)));
                        }
                        continue;
                    }
                    let Some(v) = slot.int(bytes) else { continue };
                    if is_dialog {
                        if let Some(t) = talks {
                            if v > 0 && !t.by_id.contains_key(&(v as u32)) {
                                c.add(self.problem(Kind::MissingDialog, list, row, at, format!("id_dialog = {v}: the file has no such dialog")));
                            }
                        }
                    } else if is_path {
                        if let Some(p) = paths {
                            if v > 0 && p.get(v as u32).is_none() {
                                c.add(self.problem(Kind::MissingPath, list, row, at, format!("{} = {v} is not in path.data", slot.path)));
                            }
                        }
                    } else if let Some(set) = set {
                        // For masks: the unnamed bits; all bits set means "every one".
                        let Some(key) = unnamed_set_value(set, v, all_bits) else { continue };
                        match odd.iter_mut().find(|(k, _, _)| *k == key) {
                            Some(entry) => entry.2 += 1,
                            None => odd.push((key, row, 1)),
                        }
                    }
                }
                let Some(set) = set else { continue };
                let set_name = slot.set.as_deref().unwrap_or("");
                for (key, row, records) in odd {
                    let in_records = if records > 1 { format!(" in {records} records") } else { String::new() };
                    let (kind, message) = if set.flags {
                        let bits: Vec<String> = (0..64).filter(|b| key & (1u64 << b) != 0).map(|b| b.to_string()).collect();
                        let s = if bits.len() > 1 { "s" } else { "" };
                        (Kind::UnnamedBits, format!("{}: bit{s} {} set{in_records}, not named by {set_name}", slot.path, bits.join(", ")))
                    } else {
                        (Kind::UnknownEnum, format!("{} = {}{in_records}, not a value of {set_name}", slot.path, key as i64))
                    };
                    c.add(self.problem(kind, list, row, Some((slot.path.as_str(), slot.off)), message));
                }
            }
        }
    }

    /// Dialog options that open windows the dialog does not have.
    fn check_dialogs(&self, c: &mut Collector) {
        let Ok(data) = self.talk_data() else { return };
        for (index, t) in data.talks.iter().enumerate() {
            let windows: std::collections::HashSet<u32> = t.windows.iter().map(|w| w.id).collect();
            for w in &t.windows {
                for o in &w.options {
                    if o.id & 0x8000_0000 == 0 && !windows.contains(&o.id) {
                        let title = t.title();
                        c.add(Problem {
                            kind: Kind::DialogWindow,
                            list: None,
                            row: Some(index),
                            id: t.id,
                            name: if title.is_empty() { format!("Dialog {}", t.id) } else { title },
                            icon: None,
                            field: Some(format!("window {} › “{}”", w.id, o.text)),
                            off: None,
                            message: format!("Option “{}” of window {} opens window {}, which the dialog does not have", o.text, w.id, o.id),
                            talk: Some(index),
                        });
                    }
                }
            }
        }
    }

    /// Lists read with a borrowed, partial or missing layout.
    fn check_layouts(&self, c: &mut Collector) {
        for (list, r) in self.lists.iter().enumerate() {
            let block = &self.file.lists[list];
            if block.count == 0 || r.fit == LayoutFit::Exact {
                continue;
            }
            let how = match r.fit {
                LayoutFit::Partial => "the layout covers only part of each record",
                LayoutFit::Borrowed => "fields borrowed from another version (same record size)",
                LayoutFit::Grown => "fields borrowed from an older, smaller struct",
                LayoutFit::Named => "only the name is known",
                _ => "no layout",
            };
            c.add(Problem {
                kind: Kind::Layout,
                list: Some(list),
                row: None,
                id: 0,
                name: self.list_name(list),
                icon: None,
                field: None,
                off: None,
                message: format!("{} records of {} bytes: {how}", block.count, block.item_size),
                talk: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(flags: bool, values: &[i64]) -> EnumSet {
        EnumSet {
            label: String::new(),
            flags,
            items: values.iter().map(|value| (value.to_string(), String::new())).collect(),
            descriptions: HashMap::new(),
        }
    }

    #[test]
    fn zero_is_valid_when_an_enum_names_it() {
        let values = set(false, &[0, 1, 2]);
        assert_eq!(unnamed_set_value(&values, 0, u32::MAX as u64), None);
        assert_eq!(unnamed_set_value(&values, 3, u32::MAX as u64), Some(3));

        let flags = set(true, &[1]);
        assert_eq!(unnamed_set_value(&flags, 0, u32::MAX as u64), None);
        assert_eq!(unnamed_set_value(&flags, 1, u32::MAX as u64), None);
        assert_eq!(unnamed_set_value(&flags, 2, u32::MAX as u64), Some(2));
    }
}
