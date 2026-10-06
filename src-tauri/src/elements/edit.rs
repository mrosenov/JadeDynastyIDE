//! Editing records in memory, with undo and redo.
//!
//! Every edit is a list of operations on the open file's data: setting bytes
//! of a record, inserting a record (a clone) or removing one. Each keeps what
//! it replaced, so undo and redo replay exact bytes.
//!
//! Records are tracked by a stable uid rather than their row, since inserting
//! and removing records moves the rows after them. The first time a record of
//! the opened file is touched its original bytes are kept: it counts as
//! changed while it differs from them, as deleted while it is gone, and a
//! clone counts as added.
//!
//! Values come in as text and are checked against the field's type in that
//! record (conditional types applied): integer ranges, floats and texts that
//! fit their fixed-size field. A text may fill every slot without a terminator,
//! as records produced by the official tools sometimes do.
//!
//! The history lists every edit with its time and what it changed; one edit
//! can be reverted on its own, asking first when later edits changed the same
//! bytes.

use std::collections::{HashMap, HashSet};

use encoding_rs::GBK;
use serde::{Deserialize, Serialize};

use super::format::Ty;
use super::refs::registry_space;
use super::{search, Document};

#[derive(Debug, Clone)]
enum Op {
    /// Bytes of a record set, at `off` within it.
    Set { list: usize, uid: u64, off: usize, old: Vec<u8>, new: Vec<u8> },
    /// A record inserted at `row`.
    Insert { list: usize, uid: u64, row: usize, bytes: Vec<u8> },
    /// The record at `row` removed.
    Remove { list: usize, uid: u64, row: usize, bytes: Vec<u8> },
    /// One TALK_PROC replaced by index. Its links and numeric fields are
    /// unchanged; only its encoded human-facing strings may differ.
    Talk { index: usize, old: Vec<u8>, new: Vec<u8> },
}

impl Op {
    fn inverse(&self) -> Op {
        match self.clone() {
            Op::Set { list, uid, off, old, new } => Op::Set { list, uid, off, old: new, new: old },
            Op::Insert { list, uid, row, bytes } => Op::Remove { list, uid, row, bytes },
            Op::Remove { list, uid, row, bytes } => Op::Insert { list, uid, row, bytes },
            Op::Talk { index, old, new } => Op::Talk { index, old: new, new: old },
        }
    }

    fn record(&self) -> Option<(usize, u64)> {
        match self {
            Op::Set { list, uid, .. } | Op::Insert { list, uid, .. } | Op::Remove { list, uid, .. } => Some((*list, *uid)),
            Op::Talk { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Entry {
    id: u64,
    label: String,
    /// Unix time in milliseconds.
    time: u64,
    ops: Vec<Op>,
    /// The edit this one reverts (from the history).
    reverts: Option<u64>,
}

/// The edits made to the open file.
#[derive(Debug, Clone, Default)]
pub struct Journal {
    done: Vec<Entry>,
    undone: Vec<Entry>,
    /// Uid of each record, per list and row.
    rows: Vec<Vec<u64>>,
    /// Uids of each list's records as the file was opened.
    initial: Vec<Vec<u64>>,
    /// Original bytes (and list) of every record of the opened file ever touched.
    originals: HashMap<u64, (usize, Vec<u8>)>,
    /// Original encoded bytes of dialogs touched since opening or saving.
    talk_originals: HashMap<usize, Vec<u8>>,
    /// Records created by edits (clones).
    born: HashSet<u64>,
    next_uid: u64,
    next_id: u64,
    /// The last save: the edit it was made after (none: before any) and when.
    saved: Option<(Option<u64>, u32)>,
}

impl Journal {
    /// A journal for a file with these list sizes, records numbered in order.
    pub fn new(counts: impl IntoIterator<Item = usize>) -> Self {
        let mut next_uid = 0;
        let rows: Vec<Vec<u64>> = counts
            .into_iter()
            .map(|n| {
                let start = next_uid;
                next_uid += n as u64;
                (start..next_uid).collect()
            })
            .collect();
        Journal { initial: rows.clone(), rows, next_uid, ..Default::default() }
    }

    /// The file was saved: edits count from it from now on (markers, revert
    /// all), while undo and the history keep going back past it.
    pub fn mark_saved(&mut self, time: u32) {
        self.initial = self.rows.clone();
        self.originals.clear();
        self.talk_originals.clear();
        self.born.clear();
        // Reverts are not listed in the history; the save shows after the edit before.
        self.saved = Some((self.done.iter().rev().find(|e| e.reverts.is_none()).map(|e| e.id), time));
    }

    fn row_of(&self, list: usize, uid: u64) -> Option<usize> {
        self.rows.get(list)?.iter().position(|&u| u == uid)
    }
}

/// A field one history entry changed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiff {
    pub field: String,
    pub off: usize,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecord {
    pub list: usize,
    /// The record's row now (None: it is deleted).
    pub row: Option<usize>,
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    /// "edit", "clone", "import", "copy" or "delete".
    pub action: &'static str,
    pub fields: Vec<FieldDiff>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: u64,
    pub label: String,
    pub time: u64,
    /// Undone (Redo would apply it again).
    pub undone: bool,
    /// The edit this one reverted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverts: Option<u64>,
    /// A later edit that reverted this one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverted_by: Option<u64>,
    /// When it was reverted (unix ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverted_at: Option<u64>,
    /// The file was last saved after this edit (unix seconds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<u32>,
    pub records: Vec<HistoryRecord>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// One field to set: by its offset in the record, to a value as text.
#[derive(Debug, Clone, Deserialize)]
pub struct FieldEdit {
    pub off: usize,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkWindowTextEdit {
    pub text: String,
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkTextEdit {
    pub text: String,
    pub windows: Vec<TalkWindowTextEdit>,
}

/// Rows of a list moved: from `at` on, by `delta` (+1 inserted, -1 removed).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shift {
    pub list: usize,
    pub at: usize,
    pub delta: i32,
    /// The list's record count afterwards.
    pub count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo: Option<String>,
    /// Records of the opened file that differ from it, as (list, row).
    pub changed: Vec<(usize, usize)>,
    /// Records created by edits (clones), as (list, row).
    pub added: Vec<(usize, usize)>,
    /// Records of the opened file deleted, per list.
    pub deleted: Vec<(usize, usize)>,
    /// Dialog indexes whose translated text differs from the opened/saved file.
    pub changed_talks: Vec<usize>,
    /// How the last action moved rows (insertions and removals), in order.
    pub shifts: Vec<Shift>,
    /// For a clone: where the new record is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<(usize, usize)>,
    /// When the file was last saved (unix seconds); edits count from then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_saved: Option<u32>,
}

/// Parses an integer in range for its type: decimal, negative or 0x hex.
fn int_in(value: &str, min: i128, max: i128, what: &str) -> Result<i128, String> {
    let v = value.trim();
    let parsed = match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => i128::from_str_radix(hex, 16).ok(),
        None => v.parse::<i128>().ok(),
    };
    let n = parsed.ok_or_else(|| format!("“{v}” is not a whole number"))?;
    if n < min || n > max {
        return Err(format!("{n} does not fit in {what} ({min} to {max})"));
    }
    Ok(n)
}

/// Line breaks as the game stores them: CR LF.
fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// The bytes of `value` for a field of type `ty`.
pub fn encode(ty: &Ty, value: &str) -> Result<Vec<u8>, String> {
    let le = |n: i128, size: usize| (n as u128).to_le_bytes()[..size].to_vec();
    Ok(match ty {
        Ty::I8 => le(int_in(value, i8::MIN.into(), i8::MAX.into(), "int8")?, 1),
        Ty::U8 => le(int_in(value, 0, u8::MAX.into(), "uint8")?, 1),
        Ty::Bool => match value.trim() {
            "true" | "1" => vec![1],
            "false" | "0" => vec![0],
            v => return Err(format!("“{v}” is not true or false")),
        },
        Ty::I16 => le(int_in(value, i16::MIN.into(), i16::MAX.into(), "int16")?, 2),
        Ty::U16 => le(int_in(value, 0, u16::MAX.into(), "uint16")?, 2),
        Ty::I32 => le(int_in(value, i32::MIN.into(), i32::MAX.into(), "int32")?, 4),
        Ty::U32 => le(int_in(value, 0, u32::MAX.into(), "uint32")?, 4),
        Ty::I64 => le(int_in(value, i64::MIN.into(), i64::MAX.into(), "int64")?, 8),
        Ty::U64 => le(int_in(value, 0, u64::MAX.into(), "uint64")?, 8),
        Ty::F32 | Ty::F64 => {
            let v: f64 = value.trim().parse().map_err(|_| format!("“{}” is not a number", value.trim()))?;
            if !v.is_finite() {
                return Err("The number must be finite".into());
            }
            if matches!(ty, Ty::F32) {
                if v.abs() > f32::MAX as f64 {
                    return Err(format!("{v} is too large for a float"));
                }
                (v as f32).to_le_bytes().to_vec()
            } else {
                v.to_le_bytes().to_vec()
            }
        }
        Ty::Wstr { n } => {
            let units: Vec<u16> = crlf(value).encode_utf16().collect();
            if units.len() > *n {
                return Err(format!("The text has {} characters; this field holds {n}", units.len()));
            }
            let mut out: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
            out.resize(n * 2, 0);
            out
        }
        Ty::Str { n } => {
            let text = crlf(value);
            let (bytes, _, unmappable) = GBK.encode(&text);
            if unmappable {
                return Err("The text has characters GBK cannot store".into());
            }
            if bytes.len() > *n {
                return Err(format!("The text takes {} bytes; this field holds {n}", bytes.len()));
            }
            let mut out = bytes.into_owned();
            out.resize(*n, 0);
            out
        }
        Ty::Bytes { n } => {
            let hex: String = value.chars().filter(|c| !c.is_whitespace()).collect();
            if hex.len() % 2 != 0 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("Enter hex bytes such as “0b 05 00 00”".into());
            }
            let mut out: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            if out.len() > *n {
                return Err(format!("{} bytes do not fit in {n}", out.len()));
            }
            out.resize(*n, 0);
            out
        }
        Ty::Array { .. } | Ty::Struct { .. } => return Err("Edit the members of an array or struct one by one".into()),
    })
}

impl Document {
    /// Applies validated record bytes as one journal entry, keeping exact bytes for undo.
    fn apply_records(&mut self, records: Vec<(usize, usize, Vec<u8>)>, additions: Vec<(usize, Vec<u8>)>, label: &str) -> Result<EditState, String> {
        // Validate every insertion before allocating identities or changing the journal.
        for (list, bytes) in &additions {
            if self.file.lists.get(*list).is_none_or(|block| block.item_size != bytes.len()) { return Err("Record size changed".into()); }
        }
        let mut ops = Vec::new();
        for (list, row, new) in records {
            let uid = self.uid_at(list, row)?;
            let old = self.file.record(list, row).ok_or("No such record")?.to_vec();
            if old.len() != new.len() { return Err("Record size changed".into()); }
            let slots = self.def(list).map(|(_, d)| search::slots(d, old.len())).unwrap_or_default();
            for slot in slots {
                let range = slot.off..slot.off + slot.size();
                if old[range.clone()] != new[range.clone()] {
                    ops.push(Op::Set { list, uid, off: slot.off, old: old[range.clone()].to_vec(), new: new[range].to_vec() });
                }
            }
        }
        let mut counts: Vec<_> = self.file.lists.iter().map(|b| b.count).collect();
        for (list, bytes) in additions {
            let uid = self.edits.next_uid;
            self.edits.next_uid += 1;
            self.edits.born.insert(uid);
            ops.push(Op::Insert { list, uid, row: counts[list], bytes });
            counts[list] += 1;
        }
        if ops.is_empty() { return Ok(self.edit_state()); }
        let count = ops.iter().filter_map(Op::record).collect::<HashSet<_>>().len();
        let entry = self.entry(&format!("{label} ({count} records)"), ops, None);
        Ok(self.apply(entry))
    }

    /// Applies a validated JSON import as one journal entry.
    pub(super) fn apply_import(&mut self, records: Vec<(usize, usize, Vec<u8>)>, additions: Vec<(usize, Vec<u8>)>) -> Result<EditState, String> {
        self.apply_records(records, additions, "Import")
    }

    /// Applies fields and complete records copied from the compared file.
    pub(super) fn apply_compare_copy(&mut self, records: Vec<(usize, usize, Vec<u8>)>, additions: Vec<(usize, Vec<u8>)>) -> Result<EditState, String> {
        self.apply_records(records, additions, "Copy from compared file")
    }

    /// Applies text fields copied by the translation assistant.
    pub(super) fn apply_translation(&mut self, records: Vec<(usize, usize, Vec<u8>)>, source: &str) -> Result<EditState, String> {
        self.apply_records(records, Vec::new(), &format!("Translate from {source}"))
    }

    /// The type of the field at `off` in this record: a layout field (its
    /// conditional type applied) or, for bytes no field describes, an int32.
    fn field_type(&self, list: usize, row: usize, off: usize) -> Result<Ty, String> {
        let block = self.file.lists.get(list).ok_or("No such list")?;
        let bytes = self.file.record(list, row).ok_or("No such record")?;
        let slots = self.def(list).map(|(_, d)| search::slots(d, block.item_size)).unwrap_or_default();
        if let Some(s) = slots.iter().find(|s| s.off == off) {
            return Ok(s.ty(bytes).clone());
        }
        let covered = slots.iter().any(|s| off < s.off + s.ty(bytes).size() && s.off < off + 4);
        if !covered && off % 4 == 0 && off + 4 <= block.item_size {
            return Ok(Ty::I32);
        }
        Err(format!("No editable field starts at byte {off} of this record"))
    }

    /// Writes bytes into a record without journaling.
    fn write(&mut self, list: usize, row: usize, off: usize, bytes: &[u8]) {
        let at = self.file.lists[list].data_offset + row * self.file.lists[list].item_size + off;
        self.file.data[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// Caches built from record bytes are stale after an edit.
    fn forget_caches(&mut self, lists: impl IntoIterator<Item = usize>) {
        for list in lists {
            if let Some(ids) = self.ids.get_mut(list) {
                ids.take();
            }
        }
        self.find_index.take();
        self.talks.take();
    }

    fn uid_at(&self, list: usize, row: usize) -> Result<u64, String> {
        self.edits.rows.get(list).and_then(|r| r.get(row)).copied().ok_or_else(|| "No such record".into())
    }

    /// Keeps the bytes of a record of the opened file before its first change.
    fn keep_original(&mut self, list: usize, uid: u64, bytes: &[u8]) {
        if !self.edits.born.contains(&uid) {
            self.edits.originals.entry(uid).or_insert_with(|| (list, bytes.to_vec()));
        }
    }

    fn talk_bytes(&self, index: usize) -> Result<Vec<u8>, String> {
        let talk = self.talk_data()?.talks.get(index).ok_or("No such dialog")?;
        Ok(self.file.data[talk.offset..talk.offset + talk.size].to_vec())
    }

    fn replace_talk(&mut self, index: usize, bytes: &[u8]) -> Result<(), String> {
        let (offset, size) = {
            let talk = self.talk_data()?.talks.get(index).ok_or("No such dialog")?;
            (talk.offset, talk.size)
        };
        self.file.data.splice(offset..offset + size, bytes.iter().copied());
        let segment = self.file.segments.iter_mut().find(|s| s.kind == super::reader::SegmentKind::Talk).ok_or("This file has no NPC dialog block")?;
        segment.size = segment.size + bytes.len() - size;
        self.talks.take();
        Ok(())
    }

    /// Applies one operation; returns how it moved rows.
    fn run(&mut self, op: &Op) -> Option<Shift> {
        match op {
            Op::Set { list, uid, off, new, .. } => {
                let row = self.edits.row_of(*list, *uid).expect("edited record exists");
                let bytes = self.file.record(*list, row).unwrap().to_vec();
                self.keep_original(*list, *uid, &bytes);
                self.write(*list, row, *off, new);
                None
            }
            Op::Insert { list, uid, row, bytes } => {
                self.file.insert_record(*list, *row, bytes);
                // Record bytes move the trailing TALK_PROC block; a mixed
                // Revert all may apply a dialog operation in this same batch.
                self.talks.take();
                self.edits.rows[*list].insert(*row, *uid);
                Some(Shift { list: *list, at: *row, delta: 1, count: self.file.lists[*list].count })
            }
            Op::Remove { list, uid, row, .. } => {
                let bytes = self.file.record(*list, *row).unwrap().to_vec();
                self.keep_original(*list, *uid, &bytes);
                self.file.remove_record(*list, *row);
                self.talks.take();
                self.edits.rows[*list].remove(*row);
                Some(Shift { list: *list, at: *row, delta: -1, count: self.file.lists[*list].count })
            }
            Op::Talk { index, new, .. } => {
                let current = self.talk_bytes(*index).expect("edited dialog exists");
                self.edits.talk_originals.entry(*index).or_insert(current);
                self.replace_talk(*index, new).expect("validated dialog replacement");
                None
            }
        }
    }

    fn run_all<'a>(&mut self, ops: impl IntoIterator<Item = &'a Op>) -> Vec<Shift> {
        let mut shifts = Vec::new();
        let mut lists = Vec::new();
        for op in ops {
            if let Some((list, _)) = op.record() { lists.push(list); }
            shifts.extend(self.run(op));
        }
        self.forget_caches(lists);
        shifts
    }

    fn entry(&mut self, label: &str, ops: Vec<Op>, reverts: Option<u64>) -> Entry {
        self.edits.next_id += 1;
        Entry { id: self.edits.next_id, label: label.into(), time: now_ms(), ops, reverts }
    }

    fn apply(&mut self, entry: Entry) -> EditState {
        let shifts = self.run_all(&entry.ops);
        self.edits.done.push(entry);
        self.edits.undone.clear();
        EditState { shifts, ..self.edit_state() }
    }

    /// Sets fields of one record (one undo step). Fields already holding the
    /// value are left out; nothing changes if none differs.
    pub fn edit(&mut self, list: usize, row: usize, edits: &[FieldEdit], label: &str) -> Result<EditState, String> {
        let uid = self.uid_at(list, row)?;
        let mut ops = Vec::new();
        for e in edits {
            let ty = self.field_type(list, row, e.off)?;
            let new = encode(&ty, &e.value)?;
            let record = self.file.record(list, row).ok_or("No such record")?;
            let old = record.get(e.off..e.off + new.len()).ok_or("The field runs past the record")?.to_vec();
            if old != new {
                ops.push(Op::Set { list, uid, off: e.off, old, new });
            }
        }
        if ops.is_empty() {
            return Ok(self.edit_state());
        }
        let entry = self.entry(label, ops, None);
        Ok(self.apply(entry))
    }

    /// Changes only a dialog's title/prompt, window texts and option labels.
    /// Window/option counts and all numeric control data remain byte-for-byte
    /// the same. The whole translation is one undo step.
    pub fn edit_talk_text(&mut self, index: usize, edit: &TalkTextEdit) -> Result<EditState, String> {
        let mut talk = self.talk_data()?.talks.get(index).ok_or("No such dialog")?.clone();
        if edit.windows.len() != talk.windows.len() {
            return Err("The dialog window count changed; reopen the translation editor and try again".into());
        }
        talk.text = edit.text.clone();
        for (wi, (window, changed)) in talk.windows.iter_mut().zip(&edit.windows).enumerate() {
            if changed.options.len() != window.options.len() {
                return Err(format!("Window {} option count changed; reopen the translation editor and try again", wi + 1));
            }
            window.text = changed.text.clone();
            for (option, text) in window.options.iter_mut().zip(&changed.options) {
                option.text = text.clone();
            }
        }
        let old = self.talk_bytes(index)?;
        let new = super::talk::encode_one(&talk)?;
        if old == new { return Ok(self.edit_state()); }
        let entry = self.entry(&format!("Translate NPC dialog {}", talk.id), vec![Op::Talk { index, old, new }], None);
        Ok(self.apply(entry))
    }

    /// The ID a new record of a list gets: one more than the highest ID in
    /// the list, moved past any ID already taken in the space the client
    /// registers the list's records in (a shared ID would hide one of them).
    pub fn next_free_id(&self, list: usize) -> Result<u32, String> {
        let ids_of = |l: usize| (0..self.file.lists[l].count).map(move |r| Self::record_id(self.file.record(l, r).unwrap()));
        let max = ids_of(list).max().unwrap_or(0);
        let known = |l: usize| self.lists[l].struct_name.as_deref().filter(|s| !s.to_ascii_uppercase().starts_with("UNKNOWN")).map(registry_space);
        let taken: HashSet<u32> = match known(list) {
            Some(space) => (0..self.file.lists.len()).filter(|&l| known(l) == Some(space)).flat_map(ids_of).collect(),
            None => ids_of(list).collect(),
        };
        let mut id = max.checked_add(1).ok_or("No free ID left in this list")?;
        while taken.contains(&id) {
            id = id.checked_add(1).ok_or("No free ID left in this ID space")?;
        }
        Ok(id)
    }

    /// Copies a record to the end of its list with a new ID (one undo step).
    pub fn clone_record(&mut self, list: usize, row: usize) -> Result<EditState, String> {
        let mut bytes = self.file.record(list, row).ok_or("No such record")?.to_vec();
        if bytes.len() < 4 {
            return Err("Records of this list are too small to hold an ID".into());
        }
        let id = self.next_free_id(list)?;
        let source_id = Self::record_id(&bytes);
        bytes[..4].copy_from_slice(&id.to_le_bytes());
        let name = Self::record_name(&bytes, Self::name_field(self.def(list).map(|(_, d)| d)));
        let uid = self.edits.next_uid;
        self.edits.next_uid += 1;
        self.edits.born.insert(uid);
        let new_row = self.file.lists[list].count;
        let label = format!("Clone {} ({source_id} → {id})", if name.is_empty() { format!("#{row}") } else { name });
        let entry = self.entry(&label, vec![Op::Insert { list, uid, row: new_row, bytes }], None);
        Ok(EditState { created: Some((list, new_row)), ..self.apply(entry) })
    }

    /// Removes a record (one undo step).
    pub fn delete_record(&mut self, list: usize, row: usize) -> Result<EditState, String> {
        let uid = self.uid_at(list, row)?;
        let bytes = self.file.record(list, row).ok_or("No such record")?.to_vec();
        let name = Self::record_name(&bytes, Self::name_field(self.def(list).map(|(_, d)| d)));
        let label = format!("Delete {} ({})", if name.is_empty() { format!("#{row}") } else { name }, Self::record_id(&bytes));
        let entry = self.entry(&label, vec![Op::Remove { list, uid, row, bytes }], None);
        Ok(self.apply(entry))
    }

    pub fn undo(&mut self) -> EditState {
        let mut shifts = Vec::new();
        if let Some(entry) = self.edits.done.pop() {
            let inverse: Vec<Op> = entry.ops.iter().rev().map(Op::inverse).collect();
            shifts = self.run_all(&inverse);
            self.edits.undone.push(entry);
        }
        EditState { shifts, ..self.edit_state() }
    }

    pub fn redo(&mut self) -> EditState {
        let mut shifts = Vec::new();
        if let Some(entry) = self.edits.undone.pop() {
            shifts = self.run_all(&entry.ops);
            self.edits.done.push(entry);
        }
        EditState { shifts, ..self.edit_state() }
    }

    /// Puts records back as the file was opened (one undo step): the given
    /// ones (a clone among them is removed), or with none, the whole file —
    /// clones removed, deleted records back in place, changes undone.
    pub fn revert(&mut self, records: Option<&[(usize, usize)]>, label: &str) -> EditState {
        let mut ops = Vec::new();
        // Rows as the operations will find them, while they are planned.
        let mut rows = self.edits.rows.clone();
        let present_set = |uid: u64, list: usize, rows: &Vec<Vec<u64>>| rows[list].iter().position(|&u| u == uid);
        let targets: Vec<u64> = match records {
            Some(r) => r.iter().filter_map(|&(l, row)| self.edits.rows.get(l)?.get(row).copied()).collect(),
            None => self.edits.rows.iter().flatten().copied().filter(|u| self.edits.born.contains(u) || self.edits.originals.contains_key(u)).collect(),
        };
        // Clones go.
        for &uid in targets.iter().filter(|u| self.edits.born.contains(u)) {
            let list = (0..rows.len()).find(|&l| rows[l].contains(&uid)).unwrap();
            let row = present_set(uid, list, &rows).unwrap();
            let bytes = self.file.record(list, self.edits.row_of(list, uid).unwrap()).unwrap().to_vec();
            ops.push(Op::Remove { list, uid, row, bytes });
            rows[list].remove(row);
        }
        // Deleted records come back where they were (the whole file only).
        if records.is_none() {
            for (list, initial) in self.edits.initial.iter().enumerate() {
                for (i, &uid) in initial.iter().enumerate() {
                    if rows[list].contains(&uid) {
                        continue;
                    }
                    let row = initial[..i].iter().filter(|u| rows[list].contains(u)).count();
                    let bytes = self.edits.originals[&uid].1.clone();
                    ops.push(Op::Insert { list, uid, row, bytes });
                    rows[list].insert(row, uid);
                }
            }
        }
        // Changed records get their bytes back.
        for &uid in targets.iter().filter(|u| !self.edits.born.contains(u)) {
            let Some((list, original)) = self.edits.originals.get(&uid).cloned() else { continue };
            let Some(row) = self.edits.row_of(list, uid) else { continue };
            let current = self.file.record(list, row).unwrap();
            if current != original.as_slice() {
                ops.push(Op::Set { list, uid, off: 0, old: current.to_vec(), new: original });
            }
        }
        // Dialog translations are part of Revert all. A record-only revert
        // deliberately leaves them alone.
        if records.is_none() {
            let talks: Vec<_> = self.edits.talk_originals.iter().map(|(&index, bytes)| (index, bytes.clone())).collect();
            for (index, original) in talks {
                if let Ok(current) = self.talk_bytes(index) {
                    if current != original {
                        ops.push(Op::Talk { index, old: current, new: original });
                    }
                }
            }
        }
        if ops.is_empty() {
            return self.edit_state();
        }
        let entry = self.entry(label, ops, None);
        self.apply(entry)
    }

    /// Restores one dialog's strings to the opened or last-saved bytes.
    pub fn revert_talk(&mut self, index: usize, label: &str) -> Result<EditState, String> {
        let Some(original) = self.edits.talk_originals.get(&index).cloned() else { return Ok(self.edit_state()) };
        let current = self.talk_bytes(index)?;
        if current == original { return Ok(self.edit_state()); }
        let entry = self.entry(label, vec![Op::Talk { index, old: current, new: original }], None);
        Ok(self.apply(entry))
    }

    /// Takes back one edit of the history (its record changes, clone or
    /// delete). Unless `force`, fails when later edits changed the same
    /// bytes, naming the fields.
    pub fn revert_entry(&mut self, id: u64, force: bool) -> Result<EditState, String> {
        let entry = self.edits.done.iter().find(|e| e.id == id).ok_or("That edit is not applied (undone, or no longer in the history)")?.clone();
        // Already taken back: nothing to do (reverting never goes back and forth).
        if self.edits.done.iter().any(|e| e.reverts == Some(id)) {
            return Ok(self.edit_state());
        }
        let mut ops = Vec::new();
        let mut overwritten: Vec<String> = Vec::new();
        let mut rows = self.edits.rows.clone();
        for op in entry.ops.iter().rev() {
            match op {
                Op::Set { list, uid, off, old, new } => {
                    let Some(row) = self.edits.row_of(*list, *uid) else {
                        overwritten.push("a record deleted since".into());
                        continue;
                    };
                    let current = self.file.record(*list, row).unwrap()[*off..*off + new.len()].to_vec();
                    if &current != new {
                        let base = self.file.record(*list, row).unwrap().to_vec();
                        overwritten.extend(self.diff_fields(*list, &base, *off, new, &current).into_iter().map(|f| f.field));
                    }
                    if &current != old {
                        ops.push(Op::Set { list: *list, uid: *uid, off: *off, old: current, new: old.clone() });
                    }
                }
                Op::Insert { list, uid, bytes, .. } => {
                    let Some(row) = rows[*list].iter().position(|u| u == uid) else { continue };
                    let current = self.file.record(*list, self.edits.row_of(*list, *uid).unwrap()).unwrap().to_vec();
                    if &current != bytes {
                        overwritten.push("the clone, edited since".into());
                    }
                    ops.push(Op::Remove { list: *list, uid: *uid, row, bytes: current });
                    rows[*list].remove(row);
                }
                Op::Remove { list, uid, row, bytes } => {
                    if rows[*list].contains(uid) {
                        continue;
                    }
                    let at = (*row).min(rows[*list].len());
                    ops.push(Op::Insert { list: *list, uid: *uid, row: at, bytes: bytes.clone() });
                    rows[*list].insert(at, *uid);
                }
                Op::Talk { index, old, new } => {
                    let current = self.talk_bytes(*index)?;
                    if &current != new {
                        overwritten.push(format!("NPC dialog {} text", index));
                    }
                    if &current != old {
                        ops.push(Op::Talk { index: *index, old: current, new: old.clone() });
                    }
                }
            }
        }
        if !overwritten.is_empty() && !force {
            overwritten.dedup();
            return Err(format!("CONFLICT: later edits changed {} too", overwritten.join(", ")));
        }
        if ops.is_empty() {
            return Ok(self.edit_state());
        }
        let entry = self.entry(&format!("Revert “{}”", entry.label), ops, Some(id));
        Ok(self.apply(entry))
    }

    /// The fields whose values differ between two versions of the bytes at
    /// `off` in a record (`base` holds the rest of the record).
    fn diff_fields(&self, list: usize, base: &[u8], off: usize, old: &[u8], new: &[u8]) -> Vec<FieldDiff> {
        let with = |bytes: &[u8]| {
            let mut r = base.to_vec();
            r[off..off + bytes.len()].copy_from_slice(bytes);
            r
        };
        let (before, after) = (with(old), with(new));
        let end = off + old.len();
        let size = self.file.lists[list].item_size;
        let slots = self.def(list).map(|(_, d)| search::slots(d, size)).unwrap_or_default();
        let mut out = Vec::new();
        let mut covered = vec![false; size];
        for s in &slots {
            let s_end = s.off + s.ty(&after).size();
            if s_end <= off || s.off >= end {
                continue;
            }
            covered[s.off..s_end.min(size)].iter_mut().for_each(|c| *c = true);
            let (a, b) = (s.text(&before), s.text(&after));
            if a != b {
                out.push(FieldDiff { field: s.path.clone(), off: s.off, old: a, new: b });
            }
        }
        // Bytes no field describes: as int32 words.
        let mut at = off - off % 4;
        while at + 4 <= end.min(size) {
            if !covered[at..at + 4].iter().any(|&c| c) && before[at..at + 4] != after[at..at + 4] {
                let word = |r: &[u8]| i32::from_le_bytes(r[at..at + 4].try_into().unwrap()).to_string();
                out.push(FieldDiff { field: format!("+0x{at:04X}"), off: at, old: word(&before), new: word(&after) });
            }
            at += 4;
        }
        out
    }

    /// Every edit, newest first: undone ones (Redo order) above the applied ones.
    pub fn history(&self) -> Vec<HistoryEntry> {
        // Reverts from the history do not show as edits of their own: the edit
        // they took back shows as reverted (so reverting never loops). Undo
        // still takes a revert back.
        let reverted_by: HashMap<u64, (u64, u64)> = self.edits.done.iter().filter_map(|e| Some((e.reverts?, (e.id, e.time)))).collect();
        let describe = |e: &Entry, undone: bool| {
            let mut records: Vec<HistoryRecord> = Vec::new();
            for op in &e.ops {
                let Some((list, uid)) = op.record() else { continue };
                let row = self.edits.row_of(list, uid);
                let size = self.file.lists[list].item_size;
                // The record as it is now (or, when gone, as the edit knew it).
                let bytes: Vec<u8> = match (row, op) {
                    (Some(r), _) => self.file.record(list, r).unwrap().to_vec(),
                    (None, Op::Insert { bytes, .. } | Op::Remove { bytes, .. }) => bytes.clone(),
                    (None, Op::Set { .. }) => self.edits.originals.get(&uid).map(|o| o.1.clone()).unwrap_or_else(|| vec![0; size]),
                    (None, Op::Talk { .. }) => unreachable!(),
                };
                let (action, fields) = match op {
                    Op::Set { off, old, new, .. } => ("edit", self.diff_fields(list, &bytes, *off, old, new)),
                    Op::Insert { .. } => (
                        if e.label.starts_with("Import (") { "import" } else if e.label.starts_with("Copy from compared file (") { "copy" } else { "clone" },
                        vec![],
                    ),
                    Op::Remove { .. } => ("delete", vec![]),
                    Op::Talk { .. } => unreachable!(),
                };
                let def = self.def(list).map(|(_, d)| d);
                match records.iter_mut().find(|r| r.list == list && r.id == Self::record_id(&bytes) && r.action == action) {
                    Some(r) => r.fields.extend(fields),
                    None => records.push(HistoryRecord {
                        list,
                        row,
                        id: Self::record_id(&bytes),
                        name: Self::record_name(&bytes, Self::name_field(def)),
                        icon: self.record_icon(&bytes, Self::icon_field(def)),
                        action,
                        fields,
                    }),
                }
            }
            let by = reverted_by.get(&e.id);
            HistoryEntry {
                id: e.id,
                label: e.label.clone(),
                time: e.time,
                undone,
                reverts: e.reverts,
                saved_at: self.edits.saved.filter(|(after, _)| *after == Some(e.id)).map(|(_, t)| t),
                reverted_by: by.map(|b| b.0),
                reverted_at: by.map(|b| b.1),
                records,
            }
        };
        let mut out: Vec<HistoryEntry> = self.edits.undone.iter().filter(|e| e.reverts.is_none()).map(|e| describe(e, true)).collect();
        out.extend(self.edits.done.iter().rev().filter(|e| e.reverts.is_none()).map(|e| describe(e, false)));
        out
    }

    /// The original bytes of a record of the opened file that differs from it.
    pub fn original(&self, list: usize, row: usize) -> Option<&[u8]> {
        let uid = *self.edits.rows.get(list)?.get(row)?;
        let (_, original) = self.edits.originals.get(&uid)?;
        (self.file.record(list, row)? != original.as_slice()).then_some(original.as_slice())
    }

    /// Whether a record was created by an edit (a clone).
    pub fn is_added(&self, list: usize, row: usize) -> bool {
        self.edits.rows.get(list).and_then(|r| r.get(row)).is_some_and(|u| self.edits.born.contains(u))
    }

    pub fn edit_state(&self) -> EditState {
        let mut changed = Vec::new();
        let mut added = Vec::new();
        for (list, uids) in self.edits.rows.iter().enumerate() {
            for (row, uid) in uids.iter().enumerate() {
                if self.edits.born.contains(uid) {
                    added.push((list, row));
                } else if self.edits.originals.contains_key(uid) && self.original(list, row).is_some() {
                    changed.push((list, row));
                }
            }
        }
        let mut deleted: HashMap<usize, usize> = HashMap::new();
        for (&uid, &(list, _)) in &self.edits.originals {
            if self.edits.row_of(list, uid).is_none() {
                *deleted.entry(list).or_default() += 1;
            }
        }
        let mut deleted: Vec<(usize, usize)> = deleted.into_iter().collect();
        deleted.sort();
        let mut changed_talks: Vec<usize> = self
            .edits
            .talk_originals
            .iter()
            .filter_map(|(&index, original)| self.talk_bytes(index).ok().filter(|current| current != original).map(|_| index))
            .collect();
        changed_talks.sort_unstable();
        EditState {
            undo: self.edits.done.last().map(|e| e.label.clone()),
            redo: self.edits.undone.last().map(|e| e.label.clone()),
            changed,
            added,
            deleted,
            changed_talks,
            shifts: vec![],
            created: None,
            last_saved: self.edits.saved.map(|(_, t)| t),
        }
    }
}

// ---------------------------------------------------------------- bulk edits

/// What a bulk edit does to each record's field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BulkOp {
    /// Set to the value (a number, an enum label, or text).
    Set,
    Add,
    Subtract,
    Multiply,
    /// Turn on bits (a number or mask labels joined by "|" or ",").
    SetFlags,
    /// Turn off bits.
    ClearFlags,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkEdit {
    /// The records: every match of this search…
    pub query: search::Query,
    /// …or only these (list, row) pairs, e.g. the results picked.
    #[serde(default)]
    pub records: Option<Vec<(usize, usize)>>,
    /// A field name ("proc_type") or an exact path ("addons[2].id").
    pub field: String,
    pub op: BulkOp,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSample {
    pub list: usize,
    pub row: usize,
    pub id: u32,
    pub name: String,
    pub field: String,
    pub old: String,
    pub new: String,
    /// Enum or mask labels of the old and new values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_label: Option<String>,
    /// Why the record cannot take the new value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkReport {
    /// Records the search matched.
    pub matched: usize,
    /// Records whose field gets a new value.
    pub changing: usize,
    /// Records whose field already holds it.
    pub unchanged: usize,
    /// Records of lists without the field.
    pub skipped: usize,
    /// Lists skipped, by name.
    pub skipped_lists: Vec<String>,
    /// Records the new value does not fit (left as they are).
    pub failed: usize,
    /// A few changes, then the failures (up to 50 each).
    pub samples: Vec<BulkSample>,
    /// After applying: the edit state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<EditState>,
}

const BULK_SAMPLES: usize = 50;

/// An integer of a field's width as the text `encode` reads for its type.
fn typed_bits(ty: &Ty, bits: u64) -> String {
    let width = ty.size() * 8;
    let mask = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
    let v = bits & mask;
    let signed = matches!(ty, Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64);
    if signed && width < 64 && v >> (width - 1) & 1 == 1 {
        (v as i64 - (1i64 << width)).to_string()
    } else if signed && width == 64 {
        (v as i64).to_string()
    } else {
        v.to_string()
    }
}

/// A float written without needless digits ("1.5", "2").
fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

impl Document {
    /// Plans (and with `apply`, makes, as one undo step) a bulk edit.
    pub fn bulk_edit(&mut self, edit: &BulkEdit, apply: bool) -> Result<BulkReport, String> {
        let targets: Vec<(usize, usize)> = match &edit.records {
            Some(records) => records.iter().copied().filter(|&(l, r)| self.file.record(l, r).is_some()).collect(),
            None => self.search_limited(&edit.query, usize::MAX)?.hits.iter().map(|h| (h.list, h.row)).collect(),
        };
        let field = edit.field.trim().to_lowercase();
        if field.is_empty() {
            return Err("Name the field to change.".into());
        }
        let value = edit.value.trim();
        let mut report = BulkReport { matched: targets.len(), changing: 0, unchanged: 0, skipped: 0, skipped_lists: vec![], failed: 0, samples: vec![], state: None };
        let mut failures = Vec::new();
        let mut ops = Vec::new();
        let mut slots_of: HashMap<usize, Result<Option<search::Slot>, String>> = HashMap::new();

        for &(list, row) in &targets {
            let slot = slots_of
                .entry(list)
                .or_insert_with(|| {
                    let size = self.file.lists[list].item_size;
                    let slots = self.def(list).map(|(_, d)| search::slots(d, size)).unwrap_or_default();
                    // An exact path first, else a name that only one field has.
                    if let Some(s) = slots.iter().find(|s| s.path.eq_ignore_ascii_case(&field)) {
                        return Ok(Some(s.clone()));
                    }
                    let named: Vec<&search::Slot> = slots.iter().filter(|s| s.names(&field)).collect();
                    match named.len() {
                        0 => Ok(None),
                        1 => Ok(Some(named[0].clone())),
                        n => Err(format!("“{}” is {n} fields in {} (e.g. {}); name one by its path", edit.field.trim(), self.list_name(list), named[0].path)),
                    }
                })
                .clone()?;
            let Some(slot) = slot else {
                report.skipped += 1;
                let name = self.list_name(list);
                if !report.skipped_lists.contains(&name) {
                    report.skipped_lists.push(name);
                }
                continue;
            };
            let bytes = self.file.record(list, row).unwrap().to_vec();
            let ty = slot.ty(&bytes).clone();
            let set = slot.set.as_deref().and_then(|k| self.catalog.enum_set(None, k));
            let old_text = slot.text(&bytes);
            let new_text: Result<String, String> = (|| {
                let is_int = matches!(ty, Ty::I8 | Ty::U8 | Ty::Bool | Ty::I16 | Ty::U16 | Ty::I32 | Ty::U32 | Ty::I64 | Ty::U64);
                let is_float = matches!(ty, Ty::F32 | Ty::F64);
                Ok(match edit.op {
                    BulkOp::Set => {
                        if is_int && search::parse_int(value).is_none() {
                            // An enum or mask label.
                            let v = search::parse_value(value, set).ok_or_else(|| format!("“{value}” is not a number or a label of this field"))?;
                            typed_bits(&ty, v as i64 as u64)
                        } else {
                            value.to_string()
                        }
                    }
                    BulkOp::Add | BulkOp::Subtract | BulkOp::Multiply => {
                        if !is_int && !is_float {
                            return Err("Arithmetic needs a number field".into());
                        }
                        let by: f64 = value.parse().map_err(|_| format!("“{value}” is not a number"))?;
                        let old: f64 = old_text.parse().map_err(|_| "The field holds no number".to_string())?;
                        let new = match edit.op {
                            BulkOp::Add => old + by,
                            BulkOp::Subtract => old - by,
                            _ => old * by,
                        };
                        if is_int {
                            if new.fract() != 0.0 && edit.op != BulkOp::Multiply {
                                return Err(format!("{value} is not a whole number"));
                            }
                            number(new.round())
                        } else {
                            number(new)
                        }
                    }
                    BulkOp::SetFlags | BulkOp::ClearFlags => {
                        if !is_int {
                            return Err("Flags need an integer field".into());
                        }
                        let bits = search::parse_bits(value, set).ok_or_else(|| format!("“{value}” is not a number or labels of this mask"))?;
                        let old = slot.int(&bytes).unwrap_or(0) as u64;
                        typed_bits(&ty, if edit.op == BulkOp::SetFlags { old | bits } else { old & !bits })
                    }
                })
            })();
            let id = Self::record_id(&bytes);
            let name = Self::record_name(&bytes, Self::name_field(self.def(list).map(|(_, d)| d)));
            let label = |text: &str| text.parse::<i64>().ok().and_then(|v| set?.label_for(v));
            let sample = |new: String, error: Option<String>| BulkSample {
                list,
                row,
                id,
                name: name.clone(),
                field: slot.path.clone(),
                old_label: label(&old_text),
                new_label: label(&new),
                old: old_text.clone(),
                new,
                error,
            };
            match new_text.and_then(|t| encode(&ty, &t).map(|b| (t, b))) {
                Err(e) => {
                    report.failed += 1;
                    if failures.len() < BULK_SAMPLES {
                        failures.push(sample(String::new(), Some(e)));
                    }
                }
                Ok((_, new)) => {
                    let old = bytes[slot.off..slot.off + new.len()].to_vec();
                    if old == new {
                        report.unchanged += 1;
                        continue;
                    }
                    report.changing += 1;
                    if report.samples.len() < BULK_SAMPLES {
                        let mut after = bytes.clone();
                        after[slot.off..slot.off + new.len()].copy_from_slice(&new);
                        report.samples.push(sample(slot.text(&after), None));
                    }
                    ops.push(Op::Set { list, uid: self.uid_at(list, row)?, off: slot.off, old, new });
                }
            }
        }
        report.samples.extend(failures);

        if apply && !ops.is_empty() {
            let verb = match edit.op {
                BulkOp::Set => "=",
                BulkOp::Add => "+",
                BulkOp::Subtract => "−",
                BulkOp::Multiply => "×",
                BulkOp::SetFlags => "+=",
                BulkOp::ClearFlags => "−=",
            };
            let label = format!("Bulk: {} {verb} {} ({} records)", edit.field.trim(), value, ops.len());
            let entry = self.entry(&label, ops, None);
            report.state = Some(self.apply(entry));
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_normalises_breaks() {
        assert_eq!(crlf("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }

    #[test]
    fn encodes_by_type() {
        assert_eq!(encode(&Ty::I32, "-1").unwrap(), vec![0xff; 4]);
        assert_eq!(encode(&Ty::U32, "0x50B").unwrap(), vec![0x0b, 0x05, 0, 0]);
        assert!(encode(&Ty::U8, "256").unwrap_err().contains("does not fit"));
        assert!(encode(&Ty::U32, "-1").is_err());
        assert!(encode(&Ty::I32, "1.5").is_err());
        assert_eq!(encode(&Ty::F32, "1").unwrap(), 1f32.to_le_bytes().to_vec());
        assert_eq!(encode(&Ty::Wstr { n: 4 }, "ab").unwrap(), vec![b'a', 0, b'b', 0, 0, 0, 0, 0]);
        // Line breaks become CR LF. Full fields have no terminator, matching
        // records produced by the official tools.
        assert_eq!(encode(&Ty::Wstr { n: 8 }, "a\nb").unwrap()[..8], [b'a', 0, b'\r', 0, b'\n', 0, b'b', 0]);
        assert_eq!(encode(&Ty::Wstr { n: 3 }, "abc").unwrap(), vec![b'a', 0, b'b', 0, b'c', 0]);
        assert!(encode(&Ty::Wstr { n: 3 }, "abcd").is_err());
        assert_eq!(encode(&Ty::Str { n: 4 }, "中").unwrap(), vec![0xd6, 0xd0, 0, 0]);
        assert_eq!(encode(&Ty::Str { n: 2 }, "中").unwrap(), vec![0xd6, 0xd0]);
        assert_eq!(encode(&Ty::Bytes { n: 3 }, "0b 05").unwrap(), vec![0x0b, 0x05, 0]);
    }
}
