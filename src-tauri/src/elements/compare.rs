//! Compare two elements.data files: which records one has that the other
//! lacks, and which fields changed in the records both have.
//!
//! Lists are paired by struct name (so v160 and v165 line up although their
//! list numbers differ), else by name, else by position. Records are
//! matched by ID (the n-th record with an ID to the n-th one in the other
//! file). Fields are compared by path, so layouts that grew still compare.
//!
//! "This" is the open file, "other" the one compared with it.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{edit::EditState, refs::{registry_space, IdSpace}, search, Document};

/// Changed records listed per list (the rest are counted).
const DETAIL_LIMIT: usize = 2000;
/// Field changes listed per record.
const FIELDS_PER_RECORD: usize = 200;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSide {
    pub path: String,
    pub version: u32,
    pub timestamp: u32,
    pub file_size: usize,
    pub lists: usize,
    pub records: usize,
    pub talks: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPair {
    /// The list in this file and in the other (one may be missing).
    pub this: Option<usize>,
    pub other: Option<usize>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub struct_name: Option<String>,
    pub this_count: usize,
    pub other_count: usize,
    pub this_size: usize,
    pub other_size: usize,
    /// Records only this file has (by ID).
    pub only_this: usize,
    /// Records only the other file has.
    pub only_other: usize,
    /// Records both have whose bytes differ.
    pub changed: usize,
    /// Complete records can be copied from the compared file into this list.
    pub can_copy_records_from_other: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy_records_reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub this: FileSide,
    pub other: FileSide,
    /// Lists with differences first, then the rest.
    pub lists: Vec<ListPair>,
    /// NPC dialogs: only in this file, only in the other, changed.
    pub talks: [usize; 3],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: String,
    /// The value in this file (None: the field is not in this file's layout).
    pub this: Option<String>,
    pub other: Option<String>,
    /// Both layouts describe this field with the same byte size.
    pub copyable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRef {
    pub row: usize,
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedRecord {
    /// The record in this file (row, id, name).
    #[serde(flatten)]
    pub this: RecordRef,
    pub other_row: usize,
    /// The other file's name, when it differs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other_name: Option<String>,
    pub fields: Vec<FieldChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDiff {
    pub only_this: Vec<RecordRef>,
    pub only_other: Vec<RecordRef>,
    pub changed: Vec<ChangedRecord>,
    /// More changed records than listed.
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyFields {
    pub this_row: usize,
    pub other_row: usize,
    pub fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyRequest {
    pub this_list: usize,
    pub other_list: usize,
    #[serde(default)]
    pub fields: Vec<CopyFields>,
    /// Rows that exist only in the compared file and should be appended.
    #[serde(default)]
    pub records: Vec<usize>,
}

/// Records keyed by (ID, n-th occurrence of that ID).
fn keyed(doc: &Document, list: usize) -> HashMap<(u32, usize), usize> {
    let mut seen: HashMap<u32, usize> = HashMap::new();
    (0..doc.file.lists[list].count)
        .map(|row| {
            let id = Document::record_id(doc.file.record(list, row).unwrap());
            let n = seen.entry(id).or_default();
            *n += 1;
            ((id, *n - 1), row)
        })
        .collect()
}

fn side(doc: &Document) -> FileSide {
    FileSide {
        path: doc.path.clone(),
        version: doc.file.raw_version & 0xffff,
        timestamp: doc.file.timestamp,
        file_size: doc.file.data.len(),
        lists: doc.file.lists.len(),
        records: doc.file.lists.iter().map(|l| l.count).sum(),
        talks: doc.file.talk_count,
    }
}

impl Document {
    fn record_ref(&self, list: usize, row: usize) -> RecordRef {
        let bytes = self.file.record(list, row).unwrap_or_default();
        let def = self.def(list).map(|(_, d)| d);
        RecordRef { row, id: Self::record_id(bytes), name: Self::record_name(bytes, Self::name_field(def)), icon: self.record_icon(bytes, Self::icon_field(def)) }
    }

    /// The key lists are paired by: struct name, else list name.
    fn pair_key(&self, list: usize) -> String {
        let r = &self.lists[list];
        match &r.struct_name {
            Some(s) if !s.to_ascii_uppercase().starts_with("UNKNOWN") => format!("s:{}", s.to_ascii_uppercase()),
            _ => format!("n:{}", self.list_name(list).to_lowercase()),
        }
    }
}

/// Pairs the lists of two files.
fn pairs(a: &Document, b: &Document) -> Vec<(Option<usize>, Option<usize>)> {
    // Same list count and version: the files share a layout, pair by position.
    if a.file.lists.len() == b.file.lists.len() && a.file.raw_version == b.file.raw_version {
        return (0..a.file.lists.len()).map(|i| (Some(i), Some(i))).collect();
    }
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for j in 0..b.file.lists.len() {
        by_key.entry(b.pair_key(j)).or_default().push(j);
    }
    // n-th list with a key pairs with the other file's n-th one.
    let mut taken: HashMap<String, usize> = HashMap::new();
    let mut used = vec![false; b.file.lists.len()];
    let mut out = Vec::new();
    for i in 0..a.file.lists.len() {
        let key = a.pair_key(i);
        let n = taken.entry(key.clone()).or_default();
        let j = by_key.get(&key).and_then(|v| v.get(*n)).copied();
        *n += 1;
        if let Some(j) = j {
            used[j] = true;
        }
        out.push((Some(i), j));
    }
    out.extend(used.iter().enumerate().filter(|(_, u)| !**u).map(|(j, _)| (None, Some(j))));
    out
}

fn record_copy_compatibility(a: &Document, b: &Document, i: Option<usize>, j: Option<usize>) -> (bool, Option<String>) {
    let (Some(i), Some(j)) = (i, j) else {
        return (false, Some("The matching list does not exist in both files.".into()));
    };
    let (av, bv) = (a.file.raw_version & 0xffff, b.file.raw_version & 0xffff);
    if av != bv {
        return (false, Some(format!("Adding records requires the same elements version (open: v{av}, compared: v{bv}).")));
    }
    let (am, bm) = (a.export_list_metadata(i), b.export_list_metadata(j));
    if am.struct_name != bm.struct_name || am.record_size != bm.record_size || am.schema != bm.schema {
        return (false, Some("Adding records requires matching list layouts.".into()));
    }
    (true, None)
}

pub fn summary(a: &Document, b: &Document) -> Summary {
    let mut lists: Vec<ListPair> = pairs(a, b)
        .into_iter()
        .map(|(i, j)| {
            let name = i.map(|i| a.list_name(i)).or_else(|| j.map(|j| b.list_name(j))).unwrap_or_default();
            let struct_name = i.and_then(|i| a.lists[i].struct_name.clone()).or_else(|| j.and_then(|j| b.lists[j].struct_name.clone()));
            let count = |d: &Document, l: Option<usize>| l.map_or(0, |l| d.file.lists[l].count);
            let size = |d: &Document, l: Option<usize>| l.map_or(0, |l| d.file.lists[l].item_size);
            let (only_this, only_other, changed) = match (i, j) {
                (Some(i), Some(j)) => {
                    let ka = keyed(a, i);
                    let kb = keyed(b, j);
                    let only_this = ka.keys().filter(|k| !kb.contains_key(k)).count();
                    let only_other = kb.keys().filter(|k| !ka.contains_key(k)).count();
                    let changed = ka.iter().filter(|(k, &r)| kb.get(k).is_some_and(|&s| a.file.record(i, r) != b.file.record(j, s))).count();
                    (only_this, only_other, changed)
                }
                (Some(i), None) => (a.file.lists[i].count, 0, 0),
                (None, Some(j)) => (0, b.file.lists[j].count, 0),
                (None, None) => (0, 0, 0),
            };
            let (can_copy_records_from_other, copy_records_reason) = record_copy_compatibility(a, b, i, j);
            ListPair {
                this: i,
                other: j,
                name,
                struct_name,
                this_count: count(a, i),
                other_count: count(b, j),
                this_size: size(a, i),
                other_size: size(b, j),
                only_this,
                only_other,
                changed,
                can_copy_records_from_other,
                copy_records_reason,
            }
        })
        .collect();
    // Lists with differences first, in file order.
    lists.sort_by_key(|l| (l.only_this + l.only_other + l.changed == 0, l.this.unwrap_or(usize::MAX), l.other));
    Summary { this: side(a), other: side(b), lists, talks: talk_counts(a, b) }
}

fn slot_partners(a: &Document, b: &Document, i: usize, j: usize) -> (Vec<search::Slot>, Vec<search::Slot>, Vec<Option<usize>>, Vec<bool>) {
    let slots_a = a.def(i).map(|(_, d)| search::slots(d, a.file.lists[i].item_size)).unwrap_or_default();
    let slots_b = b.def(j).map(|(_, d)| search::slots(d, b.file.lists[j].item_size)).unwrap_or_default();
    // Fields pair by path (case-insensitive: layouts spell "ID" and "id"),
    // else by the same offset and size (layouts naming a field differently).
    let by_path_b: HashMap<String, usize> = slots_b.iter().enumerate().map(|(k, s)| (s.path.to_lowercase(), k)).collect();
    let mut partner: Vec<Option<usize>> = slots_a.iter().map(|s| by_path_b.get(&s.path.to_lowercase()).copied()).collect();
    let mut matched = vec![false; slots_b.len()];
    partner.iter().flatten().for_each(|&k| matched[k] = true);
    for (n, sa) in slots_a.iter().enumerate() {
        if partner[n].is_some() {
            continue;
        }
        if let Some(k) = slots_b.iter().position(|sb| sb.off == sa.off && sb.size() == sa.size()).filter(|&k| !matched[k]) {
            partner[n] = Some(k);
            matched[k] = true;
        }
    }
    (slots_a, slots_b, partner, matched)
}

fn talk_counts(a: &Document, b: &Document) -> [usize; 3] {
    let (Ok(ta), Ok(tb)) = (a.talk_data(), b.talk_data()) else { return [0, 0, 0] };
    let only_this = ta.talks.iter().filter(|t| !tb.by_id.contains_key(&t.id)).count();
    let only_other = tb.talks.iter().filter(|t| !ta.by_id.contains_key(&t.id)).count();
    let changed = ta
        .talks
        .iter()
        .filter(|t| tb.by_id.get(&t.id).is_some_and(|&j| {
            let u = &tb.talks[j];
            let at = &a.file.data[t.offset..t.offset + t.size];
            let bt = &b.file.data[u.offset..u.offset + u.size];
            at != bt
        }))
        .count();
    [only_this, only_other, changed]
}

/// The records and fields that differ between list `i` of `a` and list `j` of `b`.
pub fn list_diff(a: &Document, b: &Document, i: Option<usize>, j: Option<usize>) -> ListDiff {
    let all = |d: &Document, l: usize| (0..d.file.lists[l].count).map(|r| d.record_ref(l, r)).collect::<Vec<_>>();
    let (i, j) = match (i, j) {
        (Some(i), Some(j)) => (i, j),
        (Some(i), None) => return ListDiff { only_this: all(a, i), only_other: vec![], changed: vec![], truncated: false },
        (None, Some(j)) => return ListDiff { only_this: vec![], only_other: all(b, j), changed: vec![], truncated: false },
        (None, None) => return ListDiff { only_this: vec![], only_other: vec![], changed: vec![], truncated: false },
    };
    let ka = keyed(a, i);
    let kb = keyed(b, j);
    let mut only_this: Vec<RecordRef> = ka.iter().filter(|(k, _)| !kb.contains_key(k)).map(|(_, &r)| a.record_ref(i, r)).collect();
    let mut only_other: Vec<RecordRef> = kb.iter().filter(|(k, _)| !ka.contains_key(k)).map(|(_, &r)| b.record_ref(j, r)).collect();
    only_this.sort_by_key(|r| r.row);
    only_other.sort_by_key(|r| r.row);

    let (slots_a, slots_b, partner, matched) = slot_partners(a, b, i, j);

    let mut pairs: Vec<(usize, usize)> = ka.iter().filter_map(|(k, &r)| kb.get(k).map(|&s| (r, s))).collect();
    pairs.sort();
    let mut changed = Vec::new();
    let mut truncated = false;
    for (r, s) in pairs {
        let (ra, rb) = (a.file.record(i, r).unwrap(), b.file.record(j, s).unwrap());
        if ra == rb {
            continue;
        }
        if changed.len() >= DETAIL_LIMIT {
            truncated = true;
            break;
        }
        let mut fields = Vec::new();
        for (sa, k) in slots_a.iter().zip(&partner) {
            let va = sa.text(ra);
            match k {
                Some(k) => {
                    let vb = slots_b[*k].text(rb);
                    if va != vb {
                        fields.push(FieldChange { field: sa.path.clone(), this: Some(va), other: Some(vb), copyable: sa.ty(ra).size() == slots_b[*k].ty(rb).size() });
                    }
                }
                None => fields.push(FieldChange { field: sa.path.clone(), this: Some(va), other: None, copyable: false }),
            }
        }
        for (_, sb) in slots_b.iter().enumerate().filter(|(k, _)| !matched[*k]) {
            fields.push(FieldChange { field: sb.path.clone(), this: None, other: Some(sb.text(rb)), copyable: false });
        }
        // No layout (or only undescribed bytes differ): compare 4-byte words.
        if fields.is_empty() {
            let n = ra.len().max(rb.len());
            for off in (0..n).step_by(4) {
                let wa = ra.get(off..off + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()));
                let wb = rb.get(off..off + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()));
                if wa != wb {
                    fields.push(FieldChange { field: format!("+0x{off:04X}"), this: wa.map(|w| w.to_string()), other: wb.map(|w| w.to_string()), copyable: false });
                }
            }
        }
        fields.truncate(FIELDS_PER_RECORD);
        let this = a.record_ref(i, r);
        let other_name = Document::record_name(rb, Document::name_field(b.def(j).map(|(_, d)| d)));
        changed.push(ChangedRecord { other_name: (other_name != this.name).then_some(other_name), this, other_row: s, fields });
    }
    ListDiff { only_this, only_other, changed, truncated }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum IdPool { Known(IdSpace), List(usize) }

fn id_pool(doc: &Document, list: usize) -> IdPool {
    doc.lists[list]
        .struct_name
        .as_deref()
        .filter(|s| !s.to_ascii_uppercase().starts_with("UNKNOWN"))
        .map(registry_space)
        .map_or(IdPool::List(list), IdPool::Known)
}

/// Copies selected compatible fields and complete missing records from the compared file.
pub fn copy_selection(a: &mut Document, b: &Document, request: &CopyRequest) -> Result<EditState, String> {
    let (i, j) = (request.this_list, request.other_list);
    if a.file.lists.get(i).is_none() || b.file.lists.get(j).is_none() {
        return Err("The compared list is no longer available. Refresh the comparison.".into());
    }
    if request.fields.is_empty() && request.records.is_empty() {
        return Err("Select at least one field or record to copy.".into());
    }

    let (slots_a, slots_b, partners, _) = slot_partners(a, b, i, j);
    let mut updates = Vec::new();
    let mut target_rows = HashSet::new();
    for selected in &request.fields {
        if !target_rows.insert(selected.this_row) {
            return Err("A target record was selected more than once. Refresh the comparison.".into());
        }
        let current = a.file.record(i, selected.this_row).ok_or("A target record moved or was removed. Refresh the comparison.")?;
        let source = b.file.record(j, selected.other_row).ok_or("A source record moved or was removed. Refresh the comparison.")?;
        if Document::record_id(current) != Document::record_id(source) {
            return Err("The selected records no longer have the same ID. Refresh the comparison.".into());
        }
        let mut bytes = current.to_vec();
        let mut names = HashSet::new();
        for field in &selected.fields {
            let key = field.to_lowercase();
            if !names.insert(key.clone()) {
                continue;
            }
            let n = slots_a.iter().position(|s| s.path.to_lowercase() == key).ok_or_else(|| format!("Field “{field}” is not present in the open file."))?;
            let k = partners[n].ok_or_else(|| format!("Field “{field}” has no compatible field in the compared file."))?;
            let (target, other) = (&slots_a[n], &slots_b[k]);
            let (target_size, source_size) = (target.ty(current).size(), other.ty(source).size());
            if target_size != source_size {
                return Err(format!("Field “{field}” has a different size in the two records and cannot be copied."));
            }
            bytes[target.off..target.off + target_size].copy_from_slice(&source[other.off..other.off + source_size]);
        }
        updates.push((i, selected.this_row, bytes));
    }

    let mut additions = Vec::new();
    if !request.records.is_empty() {
        let (compatible, reason) = record_copy_compatibility(a, b, Some(i), Some(j));
        if !compatible {
            return Err(reason.unwrap_or_else(|| "The record layouts do not match.".into()));
        }
        let mut rows = HashSet::new();
        let mut pending = HashSet::new();
        let pool = id_pool(a, i);
        for &row in &request.records {
            if !rows.insert(row) {
                continue;
            }
            let bytes = b.file.record(j, row).ok_or("A source record moved or was removed. Refresh the comparison.")?.to_vec();
            let id = Document::record_id(&bytes);
            let taken = (0..a.file.lists.len()).filter(|&list| id_pool(a, list) == pool).any(|list| {
                (0..a.file.lists[list].count).any(|r| Document::record_id(a.file.record(list, r).unwrap()) == id)
            });
            if taken || !pending.insert((pool, id)) {
                return Err(format!("ID {id} is already used in the destination ID space."));
            }
            additions.push((i, bytes));
        }
    }
    a.apply_compare_copy(updates, additions)
}

/// The differences as Markdown, for patch notes. `other_is_older`: the other
/// file is the earlier version (its-only records were removed).
pub fn markdown(a: &Document, b: &Document, other_is_older: bool) -> String {
    let s = summary(a, b);
    let (new, old) = if other_is_older { (&s.this, &s.other) } else { (&s.other, &s.this) };
    let file = |p: &str| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string();
    let mut out = format!(
        "# elements.data changes\n\n{} (v{}) → {} (v{})\n\n",
        file(&old.path),
        old.version,
        file(&new.path),
        new.version
    );
    let name = |r: &RecordRef| if r.name.is_empty() { format!("#{}", r.row) } else { r.name.clone() };
    for l in s.lists.iter().filter(|l| l.only_this + l.only_other + l.changed > 0) {
        let d = list_diff(a, b, l.this, l.other);
        let (added, removed) = if other_is_older { (&d.only_this, &d.only_other) } else { (&d.only_other, &d.only_this) };
        out += &format!("## {}\n\n", l.name);
        if !added.is_empty() {
            out += &format!("**Added ({})**\n\n", added.len());
            for r in added {
                out += &format!("- {} ({})\n", name(r), r.id);
            }
            out += "\n";
        }
        if !removed.is_empty() {
            out += &format!("**Removed ({})**\n\n", removed.len());
            for r in removed {
                out += &format!("- {} ({})\n", name(r), r.id);
            }
            out += "\n";
        }
        if !d.changed.is_empty() {
            out += &format!("**Changed ({}{})**\n\n", d.changed.len(), if d.truncated { "+" } else { "" });
            for c in &d.changed {
                out += &format!("- {} ({})\n", name(&c.this), c.this.id);
                for f in &c.fields {
                    let (before, after) = if other_is_older { (&f.other, &f.this) } else { (&f.this, &f.other) };
                    let show = |v: &Option<String>| v.clone().map_or("—".into(), |v| format!("`{}`", v.replace('`', "'")));
                    out += &format!("  - {}: {} → {}\n", f.field, show(before), show(after));
                }
            }
            out += "\n";
        }
    }
    let [t_this, t_other, t_changed] = s.talks;
    if t_this + t_other + t_changed > 0 {
        let (added, removed) = if other_is_older { (t_this, t_other) } else { (t_other, t_this) };
        out += &format!("## NPC dialogs\n\n{added} added, {removed} removed, {t_changed} changed\n");
    }
    out
}
