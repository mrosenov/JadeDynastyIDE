//! Compare two elements.data files: which records one has that the other
//! lacks, and which fields changed in the records both have.
//!
//! Lists are paired by struct name (so v160 and v165 line up although their
//! list numbers differ), else by name, else by position. Records are
//! matched by ID (the n-th record with an ID to the n-th one in the other
//! file). Fields are compared by path, so layouts that grew still compare.
//!
//! "This" is the open file, "other" the one compared with it.

use std::collections::HashMap;

use serde::Serialize;

use super::{search, Document};

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
            }
        })
        .collect();
    // Lists with differences first, in file order.
    lists.sort_by_key(|l| (l.only_this + l.only_other + l.changed == 0, l.this.unwrap_or(usize::MAX), l.other));
    Summary { this: side(a), other: side(b), lists, talks: talk_counts(a, b) }
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
                        fields.push(FieldChange { field: sa.path.clone(), this: Some(va), other: Some(vb) });
                    }
                }
                None => fields.push(FieldChange { field: sa.path.clone(), this: Some(va), other: None }),
            }
        }
        for (_, sb) in slots_b.iter().enumerate().filter(|(k, _)| !matched[*k]) {
            fields.push(FieldChange { field: sb.path.clone(), this: None, other: Some(sb.text(rb)) });
        }
        // No layout (or only undescribed bytes differ): compare 4-byte words.
        if fields.is_empty() {
            let n = ra.len().max(rb.len());
            for off in (0..n).step_by(4) {
                let wa = ra.get(off..off + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()));
                let wb = rb.get(off..off + 4).map(|w| u32::from_le_bytes(w.try_into().unwrap()));
                if wa != wb {
                    fields.push(FieldChange { field: format!("+0x{off:04X}"), this: wa.map(|w| w.to_string()), other: wb.map(|w| w.to_string()) });
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
