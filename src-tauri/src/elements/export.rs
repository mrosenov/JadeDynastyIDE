//! Export records as CSV or JSON: one record, one list, or every match of a search.
//!
//! Each record becomes a row of its scalar fields, flattened by path
//! (`addons[2].id`), after `_list`, `_listName` and `_row` (so rows can be
//! matched back on import). With labels, enum and mask fields get a
//! `field#label` column too.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{search, Document};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Csv,
    Json,
}

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

impl Document {
    pub fn export(&self, source: &Source, format: Format, labels: bool, path: &str) -> Result<Exported, String> {
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

        // Columns: the fixed ones, then each list's fields in order of first use.
        let mut slots: HashMap<usize, Vec<search::Slot>> = HashMap::new();
        let mut columns: Vec<String> = FIXED.iter().map(|s| s.to_string()).collect();
        let mut column_at: HashMap<String, usize> = HashMap::new();
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
                    if !column_at.contains_key(&name) {
                        column_at.insert(name.clone(), columns.len());
                        columns.push(name);
                    }
                }
            }
            slots.insert(list, list_slots);
        }

        let label_of = |s: &search::Slot, bytes: &[u8]| -> Option<String> {
            let v = s.int(bytes)?;
            self.catalog.enum_set(None, s.set.as_deref()?)?.label_for(v)
        };

        let out = match format {
            Format::Json => {
                let records: Vec<serde_json::Value> = rows
                    .iter()
                    .map(|&(list, row)| {
                        let bytes = self.file.record(list, row).unwrap();
                        let mut o = serde_json::Map::new();
                        o.insert("_list".into(), list.into());
                        o.insert("_listName".into(), self.list_name(list).into());
                        o.insert("_row".into(), row.into());
                        for s in &slots[&list] {
                            o.insert(s.path.clone(), s.json(bytes));
                            if labels && s.set.is_some() {
                                o.insert(format!("{}#label", s.path), label_of(s, bytes).map_or(serde_json::Value::Null, Into::into));
                            }
                        }
                        serde_json::Value::Object(o)
                    })
                    .collect();
                serde_json::to_string_pretty(&records).map_err(|e| e.to_string())? + "\n"
            }
            Format::Csv => {
                // A BOM, so spreadsheet apps read the UTF-8 (Chinese names) right.
                let mut out = String::from("\u{feff}");
                out += &columns.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",");
                out += "\r\n";
                for &(list, row) in &rows {
                    let bytes = self.file.record(list, row).unwrap();
                    let mut cells = vec![String::new(); columns.len()];
                    cells[0] = list.to_string();
                    cells[1] = self.list_name(list);
                    cells[2] = row.to_string();
                    for s in &slots[&list] {
                        cells[column_at[&s.path]] = s.text(bytes);
                        if labels && s.set.is_some() {
                            cells[column_at[&format!("{}#label", s.path)]] = label_of(s, bytes).unwrap_or_default();
                        }
                    }
                    out += &cells.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",");
                    out += "\r\n";
                }
                out
            }
        };
        std::fs::write(path, out).map_err(|e| format!("Could not write {path}: {e}"))?;
        Ok(Exported { path: path.into(), records: rows.len(), columns: columns.len() })
    }
}

/// A CSV cell, quoted when it holds a separator, quote or line break.
fn csv_cell(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::csv_cell;

    #[test]
    fn csv_cells_are_quoted_when_needed() {
        assert_eq!(csv_cell("Sword"), "Sword");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("line\r\nbreak"), "\"line\r\nbreak\"");
    }
}
