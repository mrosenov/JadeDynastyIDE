//! Copies human-facing UTF-16 text from a supported reference elements.data.
//! Lists pair by stable schema identity, records by unique ID, and fields by
//! schema path. Only text that fits the target field is proposed.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use md5::{Digest, Md5};
use serde::Serialize;

use super::{compare, edit::{encode, EditState}, format::Ty, search, Document};

const CHANGE_LIMIT: usize = 1200;
const ISSUE_LIMIT: usize = 600;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub list: usize,
    pub row: usize,
    pub id: u32,
    pub field: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub list: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReport {
    pub list: usize,
    pub source_list: Option<usize>,
    pub name: String,
    pub text_fields: usize,
    pub matched_records: usize,
    pub changed_records: usize,
    pub field_changes: usize,
    pub missing_source: usize,
    pub empty_source: usize,
    pub rejected: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub token: String,
    pub source_path: String,
    pub source_version: u32,
    pub target_version: u32,
    pub lists: Vec<ListReport>,
    pub matched_records: usize,
    pub changed_records: usize,
    pub field_changes: usize,
    pub missing_source: usize,
    pub empty_source: usize,
    pub rejected: usize,
    pub changes: Vec<Change>,
    pub issues: Vec<Issue>,
}

struct Prepared {
    report: Report,
    updates: Vec<(usize, usize, Vec<u8>)>,
}

/// ID to its row, with duplicates made ambiguous instead of silently winning.
fn unique_ids(doc: &Document, list: usize) -> HashMap<u32, Option<usize>> {
    let mut out = HashMap::new();
    for row in 0..doc.file.lists[list].count {
        let id = Document::record_id(doc.file.record(list, row).unwrap());
        out.entry(id).and_modify(|r| *r = None).or_insert(Some(row));
    }
    out
}

fn add_issue(issues: &mut Vec<Issue>, list: usize, id: Option<u32>, field: Option<String>, message: impl Into<String>) {
    if issues.len() < ISSUE_LIMIT {
        issues.push(Issue { list, id, field, message: message.into() });
    }
}

fn build(target: &Document, source: &Document) -> Result<Prepared, String> {
    if target.summary().layout_id.is_none() {
        return Err("The open file needs a supported layout before text can be translated.".into());
    }
    if source.summary().layout_id.is_none() {
        return Err("The translation source needs a supported layout.".into());
    }

    let mut reports = Vec::new();
    let mut changes = Vec::new();
    let mut issues = Vec::new();
    let mut updates = Vec::new();

    for (target_list, source_list) in compare::pairs(target, source) {
        let Some(i) = target_list else { continue };
        let target_slots = target.def(i).map(|(_, def)| search::slots(def, target.file.lists[i].item_size)).unwrap_or_default();
        let sample_count = target.file.lists[i].count.min(8);
        let text_fields = target_slots.iter().filter(|slot| {
            (0..sample_count).any(|row| target.file.record(i, row).is_some_and(|record| matches!(slot.ty(record), Ty::Wstr { .. })))
        }).count();
        if text_fields == 0 {
            continue;
        }
        let mut list_report = ListReport {
            list: i,
            source_list,
            name: target.list_name(i),
            text_fields,
            matched_records: 0,
            changed_records: 0,
            field_changes: 0,
            missing_source: 0,
            empty_source: 0,
            rejected: 0,
        };
        let Some(j) = source_list else {
            list_report.missing_source = target.file.lists[i].count;
            add_issue(&mut issues, i, None, None, "No corresponding list exists in the translation source.");
            reports.push(list_report);
            continue;
        };

        let source_slots = source.def(j).map(|(_, def)| search::slots(def, source.file.lists[j].item_size)).unwrap_or_default();
        let by_path: HashMap<String, usize> = source_slots.iter().enumerate().map(|(n, slot)| (slot.path.to_lowercase(), n)).collect();
        let target_name = Document::name_field(target.def(i).map(|(_, def)| def)).map(|(off, _)| off);
        let source_name = Document::name_field(source.def(j).map(|(_, def)| def)).map(|(off, _)| off);
        let target_ids = unique_ids(target, i);
        let source_ids = unique_ids(source, j);
        let mut ids: Vec<u32> = target_ids.keys().copied().collect();
        ids.sort_unstable();

        for id in ids {
            let Some(target_row) = target_ids.get(&id).copied().flatten() else {
                list_report.rejected += 1;
                add_issue(&mut issues, i, Some(id), None, "The target list contains this ID more than once.");
                continue;
            };
            let Some(source_row) = source_ids.get(&id).copied().flatten() else {
                list_report.missing_source += 1;
                if source_ids.get(&id).is_some() {
                    list_report.rejected += 1;
                    add_issue(&mut issues, i, Some(id), None, "The translation source contains this ID more than once.");
                }
                continue;
            };
            list_report.matched_records += 1;
            let current = target.file.record(i, target_row).unwrap();
            let other = source.file.record(j, source_row).unwrap();
            let mut translated = current.to_vec();
            let mut record_changes = 0;

            for target_slot in &target_slots {
                let target_ty = target_slot.ty(current);
                if !matches!(target_ty, Ty::Wstr { .. }) {
                    continue;
                }
                let partner = by_path
                    .get(&target_slot.path.to_lowercase())
                    .copied()
                    .or_else(|| (Some(target_slot.off) == target_name).then(|| source_name.and_then(|off| source_slots.iter().position(|slot| slot.off == off))).flatten());
                let Some(partner) = partner else { continue };
                let source_slot = &source_slots[partner];
                if !matches!(source_slot.ty(other), Ty::Wstr { .. }) {
                    continue;
                }
                let new_text = source_slot.text(other);
                if new_text.is_empty() {
                    list_report.empty_source += 1;
                    continue;
                }
                let old_text = target_slot.text(current);
                if old_text == new_text {
                    continue;
                }
                let encoded = match encode(target_ty, &new_text) {
                    Ok(encoded) => encoded,
                    Err(error) => {
                        list_report.rejected += 1;
                        add_issue(&mut issues, i, Some(id), Some(target_slot.path.clone()), error);
                        continue;
                    }
                };
                let range = target_slot.off..target_slot.off + encoded.len();
                if range.end > translated.len() {
                    list_report.rejected += 1;
                    add_issue(&mut issues, i, Some(id), Some(target_slot.path.clone()), "The target field runs past the record.");
                    continue;
                }
                translated[range].copy_from_slice(&encoded);
                record_changes += 1;
                list_report.field_changes += 1;
                if changes.len() < CHANGE_LIMIT {
                    changes.push(Change { list: i, row: target_row, id, field: target_slot.path.clone(), old: old_text, new: new_text });
                }
            }
            if record_changes > 0 {
                list_report.changed_records += 1;
                updates.push((i, target_row, translated));
            }
        }
        reports.push(list_report);
    }

    let mut hash = Md5::new();
    hash.update(b"jdide-translation-v1");
    hash.update(target.file.raw_version.to_le_bytes());
    hash.update(source.file.raw_version.to_le_bytes());
    hash.update(source.path.as_bytes());
    for (list, row, bytes) in &updates {
        hash.update(list.to_le_bytes());
        hash.update(row.to_le_bytes());
        hash.update(bytes);
    }
    let token = format!("{:x}", hash.finalize());
    let sum = |f: fn(&ListReport) -> usize| reports.iter().map(f).sum();
    let report = Report {
        token,
        source_path: source.path.clone(),
        source_version: source.file.version(),
        target_version: target.file.version(),
        matched_records: sum(|r| r.matched_records),
        changed_records: sum(|r| r.changed_records),
        field_changes: sum(|r| r.field_changes),
        missing_source: sum(|r| r.missing_source),
        empty_source: sum(|r| r.empty_source),
        rejected: sum(|r| r.rejected),
        lists: reports,
        changes,
        issues,
    };
    Ok(Prepared { report, updates })
}

pub fn preview(target: &Document, source: &Document) -> Result<Report, String> {
    Ok(build(target, source)?.report)
}

pub fn apply(target: &mut Document, source: &Document, token: &str, lists: &[usize]) -> Result<EditState, String> {
    if lists.is_empty() {
        return Err("Select at least one list to translate.".into());
    }
    let prepared = build(target, source)?;
    if prepared.report.token != token {
        return Err("The source file, open data or schemas changed after the preview. Refresh the translation preview.".into());
    }
    let selected: HashSet<usize> = lists.iter().copied().collect();
    let updates: Vec<_> = prepared.updates.into_iter().filter(|(list, _, _)| selected.contains(list)).collect();
    if updates.is_empty() {
        return Err("The selected lists have no translation changes to apply.".into());
    }
    let source_name = Path::new(&source.path).file_name().and_then(|name| name.to_str()).unwrap_or("elements.data");
    target.apply_translation(updates, source_name)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::elements::format::Catalog;

    #[test]
    fn translates_hdn_v165_names_from_forsaken_v160_by_list_and_id() {
        let target_path = "E:/Games/Elite Jade Dynasty - HDN/element/data/elements.data";
        let source_path = "E:/Games/ForsakenJD/element/data/elements.data";
        if !Path::new(target_path).is_file() || !Path::new(source_path).is_file() {
            return;
        }
        let catalog = Arc::new(Catalog::load(None));
        let mut target = Document::open(target_path.into(), catalog.clone()).unwrap();
        let source = Document::open(source_path.into(), catalog).unwrap();
        let target_name = target.records(3).unwrap().into_iter().find(|record| record.id == 55).unwrap().name;
        let source_name = source.records(3).unwrap().into_iter().find(|record| record.id == 55).unwrap().name;
        assert_eq!(target_name, source_name);
        let report = preview(&target, &source).unwrap();
        let list = report.lists.iter().find(|list| list.list == 3).unwrap();
        assert_eq!(list.source_list, Some(3));
        assert!(list.matched_records > 20_000);
        assert!(list.field_changes > 0);
        let state = apply(&mut target, &source, &report.token, &[3]).unwrap();
        assert!(state.changed.iter().any(|&(list, _)| list == 3));
    }
}
