//! Export records as versioned JSON: one record, one list, or every match of a search.
//!
//! Each record becomes an object of its scalar fields, flattened by path
//! (`addons[2].id`), after `_list`, `_listName` and `_row` (so rows can be
//! located in the original export; import matches by list and ID). With labels, enum and mask fields get a
//! `field#label` property too.

use std::collections::{HashMap, HashSet};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::{search, Document};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "from")]
pub enum Source {
    Item { list: usize, row: usize },
    List { list: usize },
    Search { query: search::Query },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: String,
    pub records: usize,
    pub columns: usize,
}

const FIXED: [&str; 3] = ["_list", "_listName", "_row"];

pub const JSON_FORMAT: &str = "jdide-elements";
pub const JSON_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonList {
    pub list: usize,
    pub struct_name: Option<String>,
    pub record_size: usize,
    pub schema: String,
}

impl Document {
    /// Byte interpretation, excluding comments, labels and other display-only metadata.
    pub(super) fn export_list_metadata(&self, list: usize) -> JsonList {
        fn shape(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(map) => {
                    for key in ["c", "e", "display", "refs", "g"] { map.remove(key); }
                    for value in map.values_mut() { shape(value); }
                }
                serde_json::Value::Array(values) => { for value in values { shape(value); } }
                _ => {}
            }
        }
        let mut fields = serde_json::to_value(self.def(list).map(|(_, d)| &d.fields)).expect("schema fields serialize");
        shape(&mut fields);
        JsonList { list, struct_name: self.lists[list].struct_name.clone(), record_size: self.file.lists[list].item_size,
            schema: format!("{:x}", Md5::digest(fields.to_string().as_bytes())) }
    }

    pub fn export(&self, source: &Source, labels: bool, path: &str) -> Result<Exported, String> {
        let rows: Vec<(usize, usize)> = match source {
            Source::Item { list, row } => {
                self.file.record(*list, *row).ok_or("No such record")?;
                vec![(*list, *row)]
            }
            Source::List { list } => {
                let block = self.file.lists.get(*list).ok_or("No such list")?;
                (0..block.count).map(|r| (*list, r)).collect()
            }
            Source::Search { query } => self.search_limited(query, usize::MAX)?.hits.iter().map(|h| (h.list, h.row)).collect(),
        };
        if rows.is_empty() {
            return Err("Nothing to export.".into());
        }

        // Count distinct field paths across all exported lists.
        let mut slots: HashMap<usize, Vec<search::Slot>> = HashMap::new();
        let mut columns: HashSet<String> = FIXED.iter().map(|s| s.to_string()).collect();
        for &(list, _) in &rows {
            if slots.contains_key(&list) {
                continue;
            }
            let list_slots = self.def(list).map(|(_, d)| search::slots(d, self.file.lists[list].item_size)).unwrap_or_default();
            for s in &list_slots {
                let mut names = vec![s.path.clone()];
                if labels && s.set.is_some() {
                    names.push(format!("{}#label", s.path));
                }
                for name in names {
                    columns.insert(name);
                }
            }
            slots.insert(list, list_slots);
        }

        let label_of = |s: &search::Slot, bytes: &[u8]| -> Option<String> {
            let v = s.int(bytes)?;
            self.catalog.enum_set(None, s.set.as_deref()?)?.label_for(v)
        };

        let records: Vec<serde_json::Value> = rows
            .iter()
            .map(|&(list, row)| {
                let bytes = self.file.record(list, row).unwrap();
                let mut o = serde_json::Map::new();
                o.insert("_list".into(), list.into());
                o.insert("_listName".into(), self.list_name(list).into());
                o.insert("_row".into(), row.into());
                // A complete source record lets imports add missing IDs without a template.
                o.insert("_raw".into(), bytes.iter().map(|b| format!("{b:02x}")).collect::<String>().into());
                for s in &slots[&list] {
                    o.insert(s.path.clone(), s.json(bytes));
                    if labels && s.set.is_some() {
                        o.insert(format!("{}#label", s.path), label_of(s, bytes).map_or(serde_json::Value::Null, Into::into));
                    }
                }
                serde_json::Value::Object(o)
            })
            .collect();
        let mut list_numbers: Vec<_> = slots.keys().copied().collect();
        list_numbers.sort_unstable();
        let lists: Vec<_> = list_numbers.into_iter().map(|list| self.export_list_metadata(list)).collect();
        let export = serde_json::json!({ "format": JSON_FORMAT, "formatVersion": JSON_FORMAT_VERSION,
            "elementsVersion": self.file.raw_version & 0xffff, "lists": lists, "records": records });
        let out = serde_json::to_string_pretty(&export).map_err(|e| e.to_string())? + "\n";
        std::fs::write(path, out).map_err(|e| format!("Could not write {path}: {e}"))?;
        Ok(Exported { path: path.into(), records: rows.len(), columns: columns.len() })
    }
}
