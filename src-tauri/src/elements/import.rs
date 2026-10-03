//! Import JD IDE exports. Versioned JSON can also add complete source records.

use std::collections::{BTreeMap, HashMap, HashSet};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use super::{edit::{encode, EditState}, export::{JsonList, JSON_FORMAT, JSON_FORMAT_VERSION}, refs::{registry_space, IdSpace}, search::{self, Slot}, Document};

#[derive(Debug)]
pub struct Input {
    rows: Vec<BTreeMap<String, serde_json::Value>>,
    digest: Vec<u8>,
    version: Option<u32>,
    lists: Vec<JsonList>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonExport {
    format: String,
    format_version: u32,
    elements_version: u32,
    lists: Vec<JsonList>,
    records: Vec<JsonRow>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Addition {
    pub source_row: usize,
    pub list: usize,
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub source_row: usize,
    pub list: usize,
    pub id: u32,
    pub field: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub source_row: usize,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub token: String,
    pub total: usize,
    pub matched: usize,
    pub changing: usize,
    pub adding: usize,
    pub source_version: Option<u32>,
    pub unchanged: usize,
    pub rejected: usize,
    pub fields: usize,
    pub changes: Vec<Change>,
    pub additions: Vec<Addition>,
    pub issues: Vec<Issue>,
    pub state: Option<EditState>,
}

// Unlike Value's object parser, reject duplicate keys instead of silently losing a value.
struct JsonRow(BTreeMap<String, serde_json::Value>);
impl<'de> Deserialize<'de> for JsonRow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = JsonRow;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("a record object") }
            fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<JsonRow, M::Error> {
                let mut row = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    if row.insert(key.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!("Duplicate column: {key}")));
                    }
                }
                Ok(JsonRow(row))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

impl Input {
    pub fn read(path: &str) -> Result<Self, String> {
        if !std::path::Path::new(path).extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case("json")) {
            return Err("Choose a .json file exported by JD IDE.".into());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {path}: {e}"))?;
        Self::parse(&text)
    }

    fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim_start_matches('\u{feff}');
        let (mut version, mut lists) = (None, Vec::new());
        let records = if text.trim_start().starts_with('[') {
            serde_json::from_str::<Vec<JsonRow>>(text).map_err(|e| format!("Invalid JSON export: {e}"))?
        } else {
            let export: JsonExport = serde_json::from_str(text).map_err(|e| format!("Invalid JSON export: {e}"))?;
            if export.format != JSON_FORMAT || export.format_version != JSON_FORMAT_VERSION {
                return Err("Unsupported JSON export format. Export the records again with this version of JD IDE.".into());
            }
            version = Some(export.elements_version);
            lists = export.lists;
            export.records
        };
        let rows: Vec<_> = records.into_iter().map(|r| r.0).collect();
        if rows.is_empty() { return Err("The import contains no records.".into()); }
        let mut hash = Md5::new();
        hash.update("json");
        hash.update(text.as_bytes());
        Ok(Self { rows, digest: hash.finalize().to_vec(), version, lists })
    }
}

fn value_text(value: &serde_json::Value) -> Result<String, String> {
    match value {
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        _ => Err("Use a scalar value; null, arrays and objects cannot be imported.".into()),
    }
}

fn integer(value: Option<&serde_json::Value>, field: &str) -> Result<u32, String> {
    value.and_then(|v| value_text(v).ok()).and_then(|s| s.trim().parse().ok()).ok_or_else(|| format!("Missing or invalid {field}; use a whole number from 0 to {}.", u32::MAX))
}

struct ListInfo {
    slots: Vec<Slot>,
    ids: HashMap<u32, Option<usize>>,
    order: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Pool { Known(IdSpace), List(usize) }

fn raw_record(row: &BTreeMap<String, serde_json::Value>, size: usize, id: u32) -> Result<Vec<u8>, String> {
    let raw = row.get("_raw").and_then(|v| v.as_str()).ok_or("Missing complete record bytes (_raw). Export this record again as JSON.")?;
    if raw.len() != size * 2 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("Invalid _raw data: expected exactly {size} bytes encoded as hex."));
    }
    let bytes: Vec<_> = (0..raw.len()).step_by(2).map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap()).collect();
    if bytes.len() < 4 || Document::record_id(&bytes) != id {
        return Err("The ID field differs from the complete source record. Keep the exported ID unchanged.".into());
    }
    Ok(bytes)
}

impl Document {
    pub fn import_records(&mut self, input: &Input, token: Option<&str>) -> Result<Report, String> {
        let mut metadata = HashMap::new();
        if let Some(version) = input.version {
            let target = self.file.raw_version & 0xffff;
            if version != target {
                return Err(format!("Cannot import elements v{version} into v{target}. Import requires the same elements version. Convert the source records to v{target} with a compatible conversion tool, then export them again. Changing the version number in the JSON does not convert the data."));
            }
            for source in &input.lists {
                if metadata.insert(source.list, source).is_some() { return Err(format!("Duplicate list metadata for list {}.", source.list)); }
            }
            // Validate all referenced list layouts before any row can be applied.
            let used: HashSet<_> = input.rows.iter().filter_map(|r| integer(r.get("_list"), "_list").ok()).map(|n| n as usize).collect();
            for list in used {
                let source = metadata.get(&list).ok_or_else(|| format!("Missing layout metadata for list {list}. Export the records again as JSON."))?;
                if list >= self.file.lists.len() { return Err(format!("List {list} is missing in the destination layout.")); }
                let target = self.export_list_metadata(list);
                if source.struct_name != target.struct_name || source.record_size != target.record_size || source.schema != target.schema {
                    return Err(format!("List {list} has a different record layout or schema, despite the matching elements version. Import requires matching layouts; convert the source records before importing."));
                }
            }
        }
        let mut lists = HashMap::new();
        let mut hash = Md5::new();
        hash.update(&input.digest);
        hash.update(self.path.as_bytes());
        hash.update(&self.file.data);
        // Include schemas: identical data can be interpreted differently after an overlay edit.
        for list in 0..self.file.lists.len() {
            if let Some((_, def)) = self.def(list) { hash.update(serde_json::to_vec(def).map_err(|e| e.to_string())?); }
        }
        let current_token = format!("{:x}", hash.finalize());
        if token.is_some_and(|t| t != current_token) {
            return Err("The import file, open data or schema changed since the preview. Refresh the preview before applying.".into());
        }
        let mut report = Report { token: current_token, total: input.rows.len(), matched: 0, changing: 0, adding: 0, source_version: input.version, unchanged: 0,
            rejected: 0, fields: 0, changes: vec![], additions: vec![], issues: vec![], state: None };
        let pools: Vec<_> = self.lists.iter().enumerate().map(|(list, def)| {
            def.struct_name.as_deref().filter(|s| !s.to_ascii_uppercase().starts_with("UNKNOWN"))
                .map_or(Pool::List(list), |s| Pool::Known(registry_space(s)))
        }).collect();
        // Resolve identities first so every occurrence of a duplicate input target is rejected.
        let mut identities = Vec::new();
        let mut occurrences = HashMap::<(usize, u32), usize>::new();
        let mut incoming = HashMap::<(Pool, u32), HashSet<usize>>::new();
        for row in &input.rows {
            let identity = (|| {
                let list = integer(row.get("_list"), "_list")? as usize;
                let block = self.file.lists.get(list).ok_or_else(|| format!("List {list} does not exist."))?;
                let name = row.get("_listName").ok_or("Missing _listName; keep the list metadata from the export.")?;
                if name.as_str() != Some(self.list_name(list).as_str()) { return Err(format!("_listName does not match list {list}; import into the same layout used for export.")); }
                let info = lists.entry(list).or_insert_with(|| {
                    let slots = self.def(list).map(|(_, d)| search::slots(d, block.item_size)).unwrap_or_default();
                    let mut ids = HashMap::new();
                    for r in 0..block.count {
                        ids.entry(Self::record_id(self.file.record(list, r).unwrap())).and_modify(|v| *v = None).or_insert(Some(r));
                    }
                    // Controlling fields must be encoded before their dependent fields.
                    let mut order = Vec::new();
                    let mut ready = HashSet::new();
                    let mut remaining: Vec<usize> = (0..slots.len()).collect();
                    while !remaining.is_empty() {
                        let before = remaining.len();
                        remaining.retain(|&i| {
                            if slots[i].control_offsets().all(|off| ready.contains(&off)) {
                                order.push(i); ready.insert(slots[i].off); false
                            } else { true }
                        });
                        if before == remaining.len() { break; }
                    }
                    ListInfo { slots, ids, order }
                });
                let id_slot = info.slots.iter().find(|s| s.off == 0 && s.path.eq_ignore_ascii_case("id")).ok_or("This list has no supported ID field.")?;
                let id = integer(row.get(&id_slot.path), &id_slot.path)?;
                *occurrences.entry((list, id)).or_default() += 1;
                incoming.entry((pools[list], id)).or_default().insert(list);
                Ok((list, id))
            })();
            identities.push(identity);
        }
        let mut updates = Vec::new();
        let mut additions = Vec::new();
        let mut occupied = HashMap::<(Pool, u32), usize>::new();
        if input.version.is_some() {
            for (list, block) in self.file.lists.iter().enumerate() {
                for row in 0..block.count {
                    occupied.insert((pools[list], Self::record_id(self.file.record(list, row).unwrap())), list);
                }
            }
        }
        for (i, (row, identity)) in input.rows.iter().zip(identities).enumerate() {
            let result: Result<(), String> = (|| {
                let (list, id) = identity?;
                if occurrences[&(list, id)] > 1 { return Err(format!("List {list}, ID {id} appears more than once in the import.")); }
                let info = &lists[&list];
                let source = if input.version.is_some() { Some(raw_record(row, self.file.lists[list].item_size, id)?) } else { None };
                let target = match info.ids.get(&id) {
                    None => {
                        let bytes = source.ok_or_else(|| format!("ID {id} was not found in list {list}. Export as versioned JSON to add missing records."))?;
                        if let Some(other) = occupied.get(&(pools[list], id)) {
                            return Err(format!("ID {id} is already used in list {other}, in the same ID space."));
                        }
                        if incoming[&(pools[list], id)].len() > 1 {
                            return Err(format!("ID {id} appears in multiple imported lists sharing one ID space."));
                        }
                        let (after, _) = prepare_row(row, &info.slots, &info.order, &bytes)?;
                        report.adding += 1;
                        if report.additions.len() < 200 {
                            let name = Self::record_name(&after, Self::name_field(self.def(list).map(|(_, d)| d)));
                            report.additions.push(Addition { source_row: i + 1, list, id, name });
                        }
                        additions.push((list, after));
                        return Ok(());
                    }
                    Some(None) => return Err(format!("ID {id} is duplicated in list {list}; the target is ambiguous.")),
                    Some(Some(r)) => *r,
                };
                report.matched += 1;
                let bytes = self.file.record(list, target).unwrap();
                let (after, changes) = prepare_row(row, &info.slots, &info.order, bytes)?;
                if changes.is_empty() { report.unchanged += 1; }
                else {
                    report.changing += 1;
                    report.fields += changes.len();
                    for (field, old, new) in changes {
                        if report.changes.len() < 200 { report.changes.push(Change { source_row: i + 1, list, id, field, old, new }); }
                    }
                    updates.push((list, target, after));
                }
                Ok(())
            })();
            if let Err(message) = result {
                report.rejected += 1;
                if report.issues.len() < 100 { report.issues.push(Issue { source_row: i + 1, message }); }
            }
        }
        if token.is_some() { report.state = Some(self.apply_import(updates, additions)?); }
        Ok(report)
    }
}

type RowChanges = Vec<(String, String, String)>;
fn prepare_row(row: &BTreeMap<String, serde_json::Value>, slots: &[Slot], order: &[usize], bytes: &[u8]) -> Result<(Vec<u8>, RowChanges), String> {
    if order.len() != slots.len() { return Err("The schema has cyclic or unsupported conditional fields.".into()); }
    let mut paths = HashMap::<&str, usize>::new();
    for slot in slots { *paths.entry(&slot.path).or_default() += 1; }
    for key in row.keys() {
        if matches!(key.as_str(), "_list" | "_listName" | "_row" | "_raw") || key.ends_with("#label") { continue; }
        let found = paths.get(key.as_str()).copied().unwrap_or(0);
        if found > 1 { return Err(format!("{key}: the schema contains duplicate field paths.")); }
        if found == 0 { return Err(format!("Unknown field: {key}")); }
    }
    let mut after = bytes.to_vec();
    let mut changes = Vec::new();
    let mut ranges: Vec<_> = slots.iter().filter(|s| row.contains_key(&s.path)).map(|s| (s.off, s.off + s.size())).collect();
    ranges.sort_unstable();
    if ranges.windows(2).any(|r| r[1].0 < r[0].1) { return Err("The imported fields overlap in this schema.".into()); }
    for &index in order {
        let slot = &slots[index];
        let Some(value) = row.get(&slot.path) else { continue; };
        let text = value_text(value).map_err(|e| format!("{}: {e}", slot.path))?;
        let end = slot.off + slot.size();
        // Retain padding, noncanonical booleans and nonfinite floats on an unchanged round trip.
        if text == slot.text(&after) { continue; }
        let new = encode(slot.ty(&after), &text).map_err(|e| format!("{}: {e}", slot.path))?;
        if new.len() != slot.size() || end > after.len() { return Err(format!("{}: invalid field size.", slot.path)); }
        after[slot.off..end].copy_from_slice(&new);
    }
    for slot in slots {
        if row.contains_key(&slot.path) && bytes[slot.off..slot.off + slot.size()] != after[slot.off..slot.off + slot.size()] {
            changes.push((slot.path.clone(), slot.text(bytes), slot.text(&after)));
        }
    }
    if Document::record_id(bytes) != Document::record_id(&after) { return Err("Record IDs are matching keys and cannot be changed by import.".into()); }
    Ok((after, changes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use super::super::{format::{Catalog, Layout, ListDef, ListSlot}, edit::FieldEdit, export::Source};
    use serde_json::json;

    fn fixture() -> Document {
        let def: ListDef = serde_json::from_value(json!({ "name": "Test items", "struct": "TEST_ESSENCE", "size": 48, "fields": [
            {"name":"id", "off":0, "t":{"k":"u32"}},
            {"name":"price", "off":4, "t":{"k":"i32"}},
            {"name":"name", "off":8, "t":{"k":"wstr", "n":8}},
            {"name":"param", "off":24, "t":{"k":"i32"}, "when":[{"field":"type", "in":[1], "t":{"k":"f32"}}]},
            {"name":"type", "off":28, "t":{"k":"i32"}},
            {"name":"blob", "off":32, "t":{"k":"bytes", "n":4}},
            {"name":"big", "off":36, "t":{"k":"u64"}},
            {"name":"flag", "off":44, "t":{"k":"bool"}}
        ]})).unwrap();
        let other: ListDef = serde_json::from_value(json!({"name":"Other items", "size":8, "fields":[
            {"name":"id", "off":0, "t":{"k":"u32"}}, {"name":"price", "off":4, "t":{"k":"i32"}}
        ]})).unwrap();
        let mut catalog = Catalog::load(None);
        catalog.layouts = vec![Arc::new(Layout { id: "test".into(), version: 999, source: "test".into(), markers: vec![],
            lists: vec![Some(ListSlot::of(def)), Some(ListSlot::of(other))], enums: HashMap::new(), list_count_unverified: false })];
        let mut data = Vec::new();
        for n in [999u32, 0, 48, 2] { data.extend(n.to_le_bytes()); }
        for id in [11u32, 22] {
            let mut bytes = vec![0u8; 48];
            bytes[..4].copy_from_slice(&id.to_le_bytes());
            bytes[4..8].copy_from_slice(&10i32.to_le_bytes());
            bytes[8..10].copy_from_slice(&('剑' as u16).to_le_bytes());
            bytes[20] = 0x67; // unused string padding must survive an unchanged export
            bytes[36..44].copy_from_slice(&u64::MAX.to_le_bytes());
            bytes[44] = 2; // noncanonical true
            data.extend(bytes);
        }
        for n in [8u32, 1, 33, 10, 0] { data.extend(n.to_le_bytes()); }
        Document::from_bytes("test.data".into(), data, Arc::new(catalog)).unwrap()
    }

    fn input(rows: serde_json::Value) -> Input { Input::parse(&rows.to_string()).unwrap() }

    fn export_json(doc: &Document, source: &Source, suffix: &str) -> serde_json::Value {
        let path = std::env::temp_dir().join(format!("jdide-transfer-{}-{suffix}.json", std::process::id()));
        doc.export(source, false, path.to_str().unwrap()).unwrap();
        let value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        value
    }

    fn new_row(template: &serde_json::Value, id: u32) -> serde_json::Value {
        let mut row = template.clone();
        let mut raw = row["_raw"].as_str().unwrap().to_string();
        raw.replace_range(..8, &id.to_le_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>());
        // Unknown bytes travel with the record, not only the named fields.
        let end = raw.len();
        raw.replace_range(end - 2.., "ab");
        row["_raw"] = raw.into();
        row["id"] = id.into();
        row
    }

    #[test]
    fn versioned_json_adds_a_hundred_records_and_updates_in_one_undo_step() {
        let mut doc = fixture();
        let before = doc.file.data.clone();
        let mut export = export_json(&doc, &Source::List { list:0 }, "hundred");
        let template = export["records"][0].clone();
        let mut rows: Vec<_> = (100..200).map(|id| new_row(&template, id)).collect();
        rows[0]["price"] = 999.into();
        rows[0]["type"] = 1.into();
        rows[0]["param"] = 1.5.into();
        let mut updated = export["records"][1].clone();
        updated["price"] = 55.into();
        rows.push(updated);
        export["records"] = rows.into();
        let source = input(export);
        let plan = doc.import_records(&source, None).unwrap();
        assert_eq!((plan.adding, plan.changing, plan.rejected), (100, 1, 0));
        assert_eq!(doc.file.data, before);
        assert!(doc.history().is_empty());
        let result = doc.import_records(&source, Some(&plan.token)).unwrap();
        let state = result.state.unwrap();
        assert_eq!((state.added.len(), state.changed.len(), state.shifts.len()), (100, 1, 100));
        assert_eq!(doc.file.lists[0].count, 102);
        for i in 0..100 {
            let bytes = doc.file.record(0, i + 2).unwrap();
            assert_eq!(Document::record_id(bytes), 100 + i as u32);
            assert_eq!(bytes[47], 0xab);
        }
        assert_eq!(Document::record_id(doc.file.record(1, 0).unwrap()), 33);
        assert_eq!(&doc.file.record(0, 2).unwrap()[4..8], &999i32.to_le_bytes());
        assert_eq!(&doc.file.record(0, 2).unwrap()[24..28], &1.5f32.to_le_bytes());
        let after = doc.file.data.clone();
        let reopened = Document::from_bytes("test.data".into(), after.clone(), doc.catalog.clone()).unwrap();
        assert_eq!(reopened.file.lists[0].count, 102);
        assert_eq!(doc.history().len(), 1);
        assert_eq!(doc.history()[0].records.iter().filter(|r| r.action == "import").count(), 100);
        let entry = doc.history()[0].id;
        doc.undo(); assert_eq!(doc.file.data, before);
        doc.redo(); assert_eq!(doc.file.data, after);
        doc.revert_entry(entry, false).unwrap(); assert_eq!(doc.file.data, before);
        doc.undo(); assert_eq!(doc.file.data, after);
        doc.revert(None, "Revert all"); assert_eq!(doc.file.data, before);
    }

    #[test]
    fn version_and_layout_mismatches_block_the_entire_import() {
        let mut doc = fixture();
        let original = export_json(&doc, &Source::List { list:0 }, "incompatible");
        let before = doc.file.data.clone();
        for key in ["version", "size", "struct", "schema", "missing", "duplicate", "format"] {
            let mut export = original.clone();
            match key {
                "version" => export["elementsVersion"] = 165.into(),
                "size" => export["lists"][0]["recordSize"] = 52.into(),
                "struct" => export["lists"][0]["structName"] = "OTHER_ESSENCE".into(),
                "schema" => export["lists"][0]["schema"] = "different".into(),
                "missing" => export["lists"] = json!([]),
                "duplicate" => export["lists"].as_array_mut().unwrap().push(original["lists"][0].clone()),
                "format" => export["formatVersion"] = 999.into(),
                _ => unreachable!(),
            }
            let parsed = Input::parse(&export.to_string());
            if key == "format" { assert!(parsed.is_err()); continue; }
            let error = doc.import_records(&parsed.unwrap(), None).unwrap_err();
            if key == "version" { assert!(error.contains("Cannot import elements v165 into v999")); }
            assert_eq!(doc.file.data, before, "{key}");
        }
    }

    #[test]
    fn additions_reject_bad_bytes_and_id_conflicts_without_partial_writes() {
        let mut doc = fixture();
        let mut export = export_json(&doc, &Source::List { list:0 }, "bad-bytes");
        let template = export["records"][0].clone();
        let mut rows = vec![new_row(&template, 100)];
        let mut short = new_row(&template, 101); short["_raw"] = "00".into(); rows.push(short);
        let mut wrong_id = new_row(&template, 102); wrong_id["id"] = 103.into(); rows.push(wrong_id);
        let mut no_raw = new_row(&template, 104); no_raw.as_object_mut().unwrap().remove("_raw"); rows.push(no_raw);
        let mut bad_field = new_row(&template, 105); bad_field["price"] = "invalid".into(); rows.push(bad_field);
        rows.push(new_row(&template, 106)); rows.push(new_row(&template, 106));
        export["records"] = rows.into();
        let source = input(export);
        let plan = doc.import_records(&source, None).unwrap();
        assert_eq!((plan.adding, plan.rejected), (1, 6));
        doc.import_records(&source, Some(&plan.token)).unwrap();
        assert_eq!(doc.file.lists[0].count, 3);

        // Two lists in the same registry cannot introduce the same ID (or shadow an existing ID).
        let mut catalog = Catalog::load(None);
        let mut layout = (*doc.catalog.layouts[0]).clone();
        let mut other = layout.list(1).unwrap().clone();
        other.struct_name = Some("OTHER_ESSENCE".into());
        layout.lists[1] = Some(ListSlot::of(other));
        catalog.layouts = vec![Arc::new(layout)];
        let mut doc = doc.reload(Arc::new(catalog)).unwrap();
        let mut a = export_json(&doc, &Source::Item { list:0, row:0 }, "collision-a");
        let b = export_json(&doc, &Source::Item { list:1, row:0 }, "collision-b");
        a["lists"].as_array_mut().unwrap().push(b["lists"][0].clone());
        a["records"] = json!([new_row(&a["records"][0], 200), new_row(&b["records"][0], 200), new_row(&a["records"][0], 33)]);
        let report = doc.import_records(&input(a), None).unwrap();
        assert_eq!((report.adding, report.rejected), (0, 3));
        assert!(report.issues.iter().any(|i| i.message.contains("already used in list 1")));
    }

    #[test]
    fn json_parses_strictly() {
        assert!(Input::read("export.csv").unwrap_err().contains(".json"));
        assert!(Input::parse("id,price\n11,20").is_err());
        for text in ["[]", "{}", "[1]", "[{\"id\":1,\"id\":2}]", "[{}] trailing"] {
            assert!(Input::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn preview_apply_undo_redo_and_revert_are_one_edit() {
        let mut doc = fixture();
        let before = doc.file.data.clone();
        let source = input(json!([
            {"_list":0,"_listName":"Test items","_row":999,"id":22,"price":20,"name":"新剑"},
            {"_list":0,"_listName":"Test items","_row":1,"id":11,"price":30,"price#label":"ignored"}
        ]));
        let plan = doc.import_records(&source, None).unwrap();
        assert_eq!((plan.matched, plan.changing, plan.fields, plan.rejected), (2, 2, 3, 0));
        assert_eq!(doc.file.data, before);
        let done = doc.import_records(&source, Some(&plan.token)).unwrap();
        assert_eq!(done.state.unwrap().changed.len(), 2);
        assert_eq!(doc.history().len(), 1);
        assert_eq!(i32::from_le_bytes(doc.file.record(0, 0).unwrap()[4..8].try_into().unwrap()), 30);
        let after = doc.file.data.clone();
        doc.undo(); assert_eq!(doc.file.data, before);
        doc.redo(); assert_eq!(doc.file.data, after);
        // Reverting an import must not overwrite a later edit to an unrelated field.
        let entry = doc.history()[0].id;
        doc.edit(0, 0, &[FieldEdit { off: 28, value: "2".into() }], "Later edit").unwrap();
        doc.revert_entry(entry, false).unwrap();
        assert_eq!(&doc.file.record(0, 0).unwrap()[4..8], &10i32.to_le_bytes());
        assert_eq!(&doc.file.record(0, 0).unwrap()[28..32], &2i32.to_le_bytes());
    }

    #[test]
    fn rejects_missing_ambiguous_and_invalid_rows_without_partial_changes() {
        let mut doc = fixture();
        let source = input(json!([
            {"_list":0,"_listName":"Test items","id":11,"price":77,"name":"This is far too long"},
            {"_list":0,"_listName":"Test items","id":22,"price":50},
            {"_list":0,"_listName":"Test items","id":999,"price":50},
            {"_list":0,"_listName":"Wrong list","id":11,"price":50},
            {"_list":999,"_listName":"Test items","id":11,"price":50}
        ]));
        let plan = doc.import_records(&source, None).unwrap();
        assert_eq!((plan.changing, plan.rejected), (1, 4));
        doc.import_records(&source, Some(&plan.token)).unwrap();
        assert_eq!(&doc.file.record(0, 0).unwrap()[4..8], &10i32.to_le_bytes());
        let duplicate = input(json!([
            {"_list":0,"_listName":"Test items","id":11,"price":1},
            {"_list":0,"_listName":"Test items","id":11,"price":2}
        ]));
        let plan = doc.import_records(&duplicate, None).unwrap();
        assert_eq!((plan.changing, plan.rejected), (0, 2));
        doc.edit(0, 1, &[FieldEdit { off:0, value:"11".into() }], "Duplicate ID").unwrap();
        let plan = doc.import_records(&input(json!([{"_list":0,"_listName":"Test items","id":11,"price":1}])), None).unwrap();
        assert!(plan.issues[0].message.contains("ambiguous"));
    }

    #[test]
    fn conditional_values_use_the_imported_controller_and_validate_types() {
        let mut doc = fixture();
        let source = input(json!([{"_list":0,"_listName":"Test items","id":11,"param":1.5,"type":1,"big":"18446744073709551614"}]));
        let plan = doc.import_records(&source, None).unwrap();
        assert_eq!((plan.changing, plan.rejected), (1, 0));
        doc.import_records(&source, Some(&plan.token)).unwrap();
        assert_eq!(&doc.file.record(0, 0).unwrap()[24..28], &1.5f32.to_le_bytes());
        for (field, value) in [("price", json!(2147483648u64)), ("price", json!("")), ("price", json!(null)), ("blob", json!("no")), ("typo", json!(1)), ("param", json!("NaN"))] {
            let mut row = json!({"_list":0,"_listName":"Test items","id":11});
            row[field] = value;
            let plan = doc.import_records(&input(json!([row])), None).unwrap();
            assert_eq!(plan.rejected, 1, "{field}");
        }
        let clear = input(json!([{"_list":0,"_listName":"Test items","id":11,"name":""}]));
        let plan = doc.import_records(&clear, None).unwrap();
        assert_eq!((plan.changing, plan.fields, plan.rejected), (1, 1, 0));
    }

    #[test]
    fn stale_preview_never_applies() {
        let mut doc = fixture();
        let source = input(json!([{"_list":0,"_listName":"Test items","id":11,"price":20}]));
        let token = doc.import_records(&source, None).unwrap().token;
        let other = input(json!([{"_list":0,"_listName":"Test items","id":11,"price":21}]));
        assert!(doc.import_records(&other, Some(&token)).is_err());
        doc.edit(0, 0, &[FieldEdit { off:4, value:"15".into() }], "Changed after preview").unwrap();
        assert!(doc.import_records(&source, Some(&token)).is_err());
        assert_eq!(&doc.file.record(0, 0).unwrap()[4..8], &15i32.to_le_bytes());
        doc.undo();
        let mut catalog = Catalog::load(None);
        let mut layout = (*doc.catalog.layouts[0]).clone();
        let mut def = layout.list(0).unwrap().clone();
        def.fields[1].t = super::super::format::Ty::F32;
        layout.lists[0] = Some(ListSlot::of(def));
        catalog.layouts = vec![Arc::new(layout)];
        let mut doc = doc.reload(Arc::new(catalog)).unwrap();
        assert!(doc.import_records(&source, Some(&token)).is_err());
    }

    fn round_trip(doc: &mut Document, source: &Source, suffix: &str) {
        let before = doc.file.data.clone();
        let path = std::env::temp_dir().join(format!("jdide-import-{}-{suffix}.json", std::process::id()));
        let path = path.to_str().unwrap();
        doc.export(source, true, path).unwrap();
        let input = Input::read(path).unwrap();
        std::fs::remove_file(path).unwrap();
        let report = doc.import_records(&input, None).unwrap();
        assert_eq!(report.changing, 0, "{suffix}: {:?}", report.changes);
        assert_eq!(report.rejected, 0, "{suffix}: {:?}", report.issues);
        assert_eq!(report.unchanged, report.total);
        doc.import_records(&input, Some(&report.token)).unwrap();
        assert_eq!(doc.file.data, before);
    }

    #[test]
    fn unchanged_exports_preserve_every_byte() {
        let mut doc = fixture();
        round_trip(&mut doc, &Source::List { list: 0 }, "synthetic");
        let query = search::Query::Conditions { conditions: vec![search::Condition { field:"price".into(), op:search::Op::Eq, value:"10".into() }], match_all:true, list:None };
        round_trip(&mut doc, &Source::Search { query }, "mixed-lists");
        assert!(doc.history().is_empty());
    }

    #[test]
    fn real_v156_exports_round_trip_and_can_be_edited() {
        let root = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/".into());
        let path = format!("{root}/Game Dev/JD/zxserver/zgame/gs/config/elements.data");
        if !std::path::Path::new(&path).exists() { eprintln!("skipping import: v156 sample missing"); return; }
        let mut doc = Document::open(path, Arc::new(Catalog::load(None))).unwrap();
        round_trip(&mut doc, &Source::List { list: 0 }, "v156-addons");
        round_trip(&mut doc, &Source::Item { list: 3, row: 0 }, "v156-equipment");
        let bytes = doc.file.record(3, 0).unwrap();
        let price = search::slots(doc.def(3).unwrap().1, bytes.len()).into_iter().find(|s| s.path == "price").unwrap();
        let new_price = price.int(bytes).unwrap() + 1;
        let source = input(json!([{"_list":3,"_listName":doc.list_name(3),"id":Document::record_id(bytes),"price":new_price}]));
        let before = doc.file.data.clone();
        let plan = doc.import_records(&source, None).unwrap();
        assert_eq!((plan.changing, plan.rejected), (1, 0));
        doc.import_records(&source, Some(&plan.token)).unwrap();
        assert_eq!(price.int(doc.file.record(3, 0).unwrap()), Some(new_price));
        doc.undo(); assert_eq!(doc.file.data, before);
    }

    #[test]
    fn real_v156_transfer_saves_and_reopens_with_a_valid_checksum() {
        use super::super::save::{SaveOptions, ChecksumStatus};
        let root = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/".into());
        let path = format!("{root}/Game Dev/JD/zxserver/zgame/gs/config/elements.data");
        let path_data = format!("{root}/Game Dev/JD/zxserver/zgame/gs/config/path.data");
        if !std::path::Path::new(&path).exists() || !std::path::Path::new(&path_data).exists() {
            eprintln!("skipping transfer: v156 sample or path.data missing"); return;
        }
        let mut source = Document::open(path.clone(), Arc::new(Catalog::load(None))).unwrap();
        let mut target = Document::from_bytes(path, source.file.data.clone(), source.catalog.clone()).unwrap();
        let (list, row) = source.clone_record(3, 0).unwrap().created.unwrap();
        let expected = source.file.record(list, row).unwrap().to_vec();
        let export = export_json(&source, &Source::Item { list, row }, "real-transfer");
        let input = input(export);
        let plan = target.import_records(&input, None).unwrap();
        assert_eq!((plan.adding, plan.changing, plan.rejected), (1, 0, 0));
        target.import_records(&input, Some(&plan.token)).unwrap();
        assert_eq!(target.file.record(list, row).unwrap(), expected);
        let output = std::env::temp_dir().join(format!("jdide-transfer-save-{}.data", std::process::id()));
        let options = SaveOptions { path: output.display().to_string(), path_data: Some(path_data), backup:false, replace_changed:false };
        target.save(&options).unwrap();
        let reopened = Document::open(output.display().to_string(), source.catalog.clone()).unwrap();
        assert_eq!(reopened.file.record(list, row).unwrap(), expected);
        assert_eq!(reopened.save_plan(&options).unwrap().checksum.status, ChecksumStatus::Valid);
        std::fs::remove_file(output).unwrap();
        assert!(target.edit_state().added.is_empty());
        target.undo();
        assert_eq!(target.file.lists[list].count, row);
        target.redo();
        assert_eq!(target.file.record(list, row).unwrap(), expected);
    }
}
