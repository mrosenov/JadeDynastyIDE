//! Editing records in memory, with undo and redo.
//!
//! Every edit writes the new bytes into the open file's data and records the
//! bytes it replaced, so undo and redo replay exact bytes. The first time a
//! record is touched its original bytes are kept: a record counts as changed
//! while it differs from them (editing a value back clears the mark).
//!
//! Values come in as text and are checked against the field's type in that
//! record (conditional types applied): integer ranges, floats, texts that fit
//! their field with room for the terminator.
//!
//! The history lists every edit with its time and the fields it changed; one
//! edit can be reverted on its own (as a new edit), asking first when later
//! edits changed the same bytes.

use std::collections::HashMap;

use encoding_rs::GBK;
use serde::{Deserialize, Serialize};

use super::format::Ty;
use super::{search, Document};

#[derive(Debug, Clone)]
struct Change {
    list: usize,
    row: usize,
    /// Offset within the record.
    off: usize,
    old: Vec<u8>,
    new: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Entry {
    id: u64,
    label: String,
    /// Unix time in milliseconds.
    time: u64,
    changes: Vec<Change>,
    /// The edit this one reverts (from the history).
    reverts: Option<u64>,
}

/// The edits made to the open file.
#[derive(Debug, Clone, Default)]
pub struct Journal {
    done: Vec<Entry>,
    undone: Vec<Entry>,
    /// Original bytes of every record ever touched.
    originals: HashMap<(usize, usize), Vec<u8>>,
    next_id: u64,
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
    pub row: usize,
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo: Option<String>,
    /// Records that differ from the file as opened, as (list, row).
    pub changed: Vec<(usize, usize)>,
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
            if units.len() >= *n {
                return Err(format!("The text has {} characters; this field holds {} (one is kept for the terminator)", units.len(), n - 1));
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
            if bytes.len() >= *n {
                return Err(format!("The text takes {} bytes; this field holds {} (one is kept for the terminator)", bytes.len(), n - 1));
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

    fn entry(&mut self, label: &str, changes: Vec<Change>, reverts: Option<u64>) -> Entry {
        self.edits.next_id += 1;
        Entry { id: self.edits.next_id, label: label.into(), time: now_ms(), changes, reverts }
    }

    fn apply(&mut self, entry: Entry) -> EditState {
        for c in &entry.changes {
            let bytes = self.file.record(c.list, c.row).unwrap().to_vec();
            self.edits.originals.entry((c.list, c.row)).or_insert(bytes);
            self.write(c.list, c.row, c.off, &c.new);
        }
        self.forget_caches(entry.changes.iter().map(|c| c.list).collect::<Vec<_>>());
        self.edits.done.push(entry);
        self.edits.undone.clear();
        self.edit_state()
    }

    /// Sets fields of one record (one undo step). Fields already holding the
    /// value are left out; nothing changes if none differs.
    pub fn edit(&mut self, list: usize, row: usize, edits: &[FieldEdit], label: &str) -> Result<EditState, String> {
        let mut changes = Vec::new();
        for e in edits {
            let ty = self.field_type(list, row, e.off)?;
            let new = encode(&ty, &e.value)?;
            let record = self.file.record(list, row).ok_or("No such record")?;
            let old = record.get(e.off..e.off + new.len()).ok_or("The field runs past the record")?.to_vec();
            if old != new {
                changes.push(Change { list, row, off: e.off, old, new });
            }
        }
        if changes.is_empty() {
            return Ok(self.edit_state());
        }
        let entry = self.entry(label, changes, None);
        Ok(self.apply(entry))
    }

    pub fn undo(&mut self) -> EditState {
        if let Some(entry) = self.edits.done.pop() {
            for c in entry.changes.iter().rev() {
                self.write(c.list, c.row, c.off, &c.old);
            }
            self.forget_caches(entry.changes.iter().map(|c| c.list).collect::<Vec<_>>());
            self.edits.undone.push(entry);
        }
        self.edit_state()
    }

    pub fn redo(&mut self) -> EditState {
        if let Some(entry) = self.edits.undone.pop() {
            for c in &entry.changes {
                self.write(c.list, c.row, c.off, &c.new);
            }
            self.forget_caches(entry.changes.iter().map(|c| c.list).collect::<Vec<_>>());
            self.edits.done.push(entry);
        }
        self.edit_state()
    }

    /// Puts records back as they were when the file was opened (one undo step).
    pub fn revert(&mut self, records: Option<&[(usize, usize)]>, label: &str) -> EditState {
        let targets: Vec<(usize, usize)> = match records {
            Some(r) => r.to_vec(),
            None => self.edits.originals.keys().copied().collect(),
        };
        let mut changes = Vec::new();
        for (list, row) in targets {
            let Some(original) = self.edits.originals.get(&(list, row)) else { continue };
            let current = self.file.record(list, row).unwrap();
            if current != original.as_slice() {
                changes.push(Change { list, row, off: 0, old: current.to_vec(), new: original.clone() });
            }
        }
        if changes.is_empty() {
            return self.edit_state();
        }
        let entry = self.entry(label, changes, None);
        self.apply(entry)
    }

    /// Takes back one edit of the history (as a new edit). Unless `force`,
    /// fails when later edits changed the same bytes, naming the fields.
    pub fn revert_entry(&mut self, id: u64, force: bool) -> Result<EditState, String> {
        let entry = self.edits.done.iter().find(|e| e.id == id).ok_or("That edit is not applied (undone, or no longer in the history)")?.clone();
        // Already taken back: nothing to do (reverting never goes back and forth).
        if self.edits.done.iter().any(|e| e.reverts == Some(id)) {
            return Ok(self.edit_state());
        }
        let mut changes = Vec::new();
        let mut overwritten = Vec::new();
        for c in &entry.changes {
            let current = self.file.record(c.list, c.row).ok_or("No such record")?[c.off..c.off + c.new.len()].to_vec();
            if current != c.new {
                overwritten.extend(self.diff_fields(c.list, c.row, c.off, &c.new, &current).into_iter().map(|f| f.field));
            }
            if current != c.old {
                changes.push(Change { list: c.list, row: c.row, off: c.off, old: current, new: c.old.clone() });
            }
        }
        if !overwritten.is_empty() && !force {
            overwritten.dedup();
            return Err(format!("CONFLICT: later edits changed {} too", overwritten.join(", ")));
        }
        if changes.is_empty() {
            return Ok(self.edit_state());
        }
        let entry = self.entry(&format!("Revert “{}”", entry.label), changes, Some(id));
        Ok(self.apply(entry))
    }

    /// The fields whose values differ between two versions of the bytes at
    /// `off` in a record (the rest of the record as it is now).
    fn diff_fields(&self, list: usize, row: usize, off: usize, old: &[u8], new: &[u8]) -> Vec<FieldDiff> {
        let Some(record) = self.file.record(list, row) else { return vec![] };
        let with = |bytes: &[u8]| {
            let mut r = record.to_vec();
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
            for c in &e.changes {
                // The fields the change set (its old bytes against its new ones).
                let fields = self.diff_fields(c.list, c.row, c.off, &c.old, &c.new);
                let bytes = self.file.record(c.list, c.row).unwrap_or_default();
                let def = self.def(c.list).map(|(_, d)| d);
                match records.iter_mut().find(|r| r.list == c.list && r.row == c.row) {
                    Some(r) => r.fields.extend(fields),
                    None => records.push(HistoryRecord {
                        list: c.list,
                        row: c.row,
                        id: Self::record_id(bytes),
                        name: Self::record_name(bytes, Self::name_field(def)),
                        icon: self.record_icon(bytes, Self::icon_field(def)),
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
                reverted_by: by.map(|b| b.0),
                reverted_at: by.map(|b| b.1),
                records,
            }
        };
        let mut out: Vec<HistoryEntry> = self.edits.undone.iter().filter(|e| e.reverts.is_none()).map(|e| describe(e, true)).collect();
        out.extend(self.edits.done.iter().rev().filter(|e| e.reverts.is_none()).map(|e| describe(e, false)));
        out
    }

    /// The original bytes of a record that differs from the file as opened.
    pub fn original(&self, list: usize, row: usize) -> Option<&[u8]> {
        let original = self.edits.originals.get(&(list, row))?;
        (self.file.record(list, row)? != original.as_slice()).then_some(original.as_slice())
    }

    pub fn edit_state(&self) -> EditState {
        let mut changed: Vec<(usize, usize)> = self.edits.originals.keys().copied().filter(|&(l, r)| self.original(l, r).is_some()).collect();
        changed.sort();
        EditState {
            undo: self.edits.done.last().map(|e| e.label.clone()),
            redo: self.edits.undone.last().map(|e| e.label.clone()),
            changed,
        }
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
        // Line breaks become CR LF; the terminator must fit.
        assert_eq!(encode(&Ty::Wstr { n: 8 }, "a\nb").unwrap()[..8], [b'a', 0, b'\r', 0, b'\n', 0, b'b', 0]);
        assert!(encode(&Ty::Wstr { n: 3 }, "abc").is_err());
        assert_eq!(encode(&Ty::Str { n: 4 }, "中").unwrap(), vec![0xd6, 0xd0, 0, 0]);
        assert_eq!(encode(&Ty::Bytes { n: 3 }, "0b 05").unwrap(), vec![0x0b, 0x05, 0]);
    }
}
