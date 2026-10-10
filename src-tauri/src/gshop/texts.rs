//! Refreshing shop item names from elements.data and descriptions from the client's item_ext_desc.txt.
//!
//! A preview lists the items whose text would change; applying writes the picked ones as one undo step
//! and refuses items whose text changed since the preview.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::layout::FieldType;
use super::{Change, Document, Entry, View};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextField {
    Name,
    Description,
}

impl TextField {
    fn meaning(self) -> &'static str {
        match self {
            TextField::Name => "name",
            TextField::Description => "description",
        }
    }

    fn get(self, item: &super::ShopItem) -> &str {
        match self {
            TextField::Name => &item.name,
            TextField::Description => &item.description,
        }
    }

    fn set(self, item: &mut super::ShopItem, text: String) {
        match self {
            TextField::Name => item.name = text,
            TextField::Description => item.description = text,
        }
    }
}

/// The shop's form of a client text: line breaks become the literal `\r` the shop stores.
pub fn shop_text(text: &str) -> String {
    text.trim_end_matches(['\n', ' ']).replace("\r\n", "\n").replace('\n', "\\r")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextUpdate {
    pub index: usize,
    pub id: u32,
    pub current: String,
    pub text: String,
    /// Why it cannot be applied (too long for the field).
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPreview {
    /// Items whose text differs.
    pub rows: Vec<TextUpdate>,
    /// Items checked, already equal, and without a source text.
    pub checked: usize,
    pub same: usize,
    pub missing: usize,
    /// Characters the field holds.
    pub limit: usize,
}

/// One picked update, with the text the item had in the preview.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextApply {
    pub index: usize,
    pub current: String,
    pub text: String,
}

impl Document {
    /// The item IDs at `picks` (or of every item), for looking up their texts.
    pub fn item_ids(&self, picks: Option<&[usize]>) -> Vec<u32> {
        match picks {
            Some(picks) => picks.iter().filter_map(|&index| self.entries.get(index).map(|entry| entry.item.id)).collect(),
            None => self.entries.iter().map(|entry| entry.item.id).collect(),
        }
    }

    /// Characters the layout stores for a text field (none: the layout lacks it).
    /// The slot size of a text field and whether it is UTF-16 (characters) or GBK (bytes).
    fn text_limit(&self, field: TextField) -> Option<(usize, bool)> {
        self.layout.leaves().into_iter().find(|leaf| leaf.meaning.as_deref() == Some(field.meaning())).and_then(|leaf| match leaf.ty {
            FieldType::Wstr { len } => Some((len, true)),
            FieldType::Str { len } => Some((len, false)),
            _ => None,
        })
    }

    /// The items (at `picks`, or all) whose text differs from `texts` (by item ID, already in the shop's form).
    pub fn text_updates(&self, field: TextField, picks: Option<&[usize]>, texts: &HashMap<u32, String>) -> Result<TextPreview, String> {
        let (limit, wide) = self.text_limit(field).ok_or_else(|| format!("This shop's item layout has no {}", field.meaning()))?;
        let indexes: Vec<usize> = match picks {
            Some(picks) => picks.iter().copied().filter(|&index| index < self.entries.len()).collect(),
            None => (0..self.entries.len()).collect(),
        };
        let mut preview = TextPreview { rows: Vec::new(), checked: indexes.len(), same: 0, missing: 0, limit };
        for index in indexes {
            let item = &self.entries[index].item;
            let Some(text) = texts.get(&item.id).filter(|text| !text.trim().is_empty()) else {
                preview.missing += 1;
                continue;
            };
            let current = field.get(item);
            if current == text {
                preview.same += 1;
                continue;
            }
            let problem = if wide {
                let length = text.encode_utf16().count();
                (length > limit).then(|| format!("{length} characters; the shop stores {limit}"))
            } else {
                let (encoded, _, unmappable) = encoding_rs::GBK.encode(text);
                if unmappable { Some("has characters GBK cannot store".to_string()) } else { (encoded.len() > limit).then(|| format!("{} bytes in GBK; the shop stores {limit}", encoded.len())) }
            };
            preview.rows.push(TextUpdate { index, id: item.id, current: current.to_string(), text: text.clone(), problem });
        }
        Ok(preview)
    }

    /// Writes the picked texts as one undo step; items whose text changed since the preview are refused.
    pub fn apply_texts(&mut self, field: TextField, rows: &[TextApply]) -> Result<View, String> {
        let mut changes = Vec::new();
        for row in rows {
            let before = self.entries.get(row.index).cloned().ok_or_else(|| format!("No item {}", row.index + 1))?;
            if field.get(&before.item) != row.current {
                return Err(format!("Item {} changed since the preview; preview again", row.index + 1));
            }
            let mut item = before.item.clone();
            field.set(&mut item, row.text.clone());
            if item != before.item {
                changes.push(Change::Item { index: row.index, before: Some(before.clone()), after: Some(Entry { item, ..before }) });
            }
        }
        if changes.is_empty() {
            return Ok(self.view());
        }
        let what = match field {
            TextField::Name => "name",
            TextField::Description => "description",
        };
        let count = changes.len();
        self.record_all(format!("Update {count} item {what}{}", if count == 1 { "" } else { "s" }), changes)
    }
}

#[cfg(test)]
mod tests {
    use super::super::layout;
    use super::*;

    #[test]
    fn descriptions_from_the_client_update_the_shop() {
        let client = std::path::Path::new("E:/Games/ForsakenJD");
        let shop = client.join("element/data/gshop.data");
        if !shop.is_file() {
            return;
        }
        let res = crate::client::Resources::new(crate::client::inspect(client).unwrap());
        let layouts = layout::builtin();
        let folder = std::env::temp_dir().join(format!("jdide-gshop-texts-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("gshop.data");
        std::fs::copy(&shop, &path).unwrap();
        let mut document = Document::open(&path, &layouts, None).unwrap();
        let texts: HashMap<u32, String> = document.item_ids(None).into_iter().filter_map(|id| Some((id, shop_text(&res.text(crate::client::table::ITEM_DESC, id)?)))).collect();
        assert!(!texts.is_empty());
        assert!(texts.values().all(|text| !text.contains('\n')), "line breaks are stored as \\r");
        let preview = document.text_updates(TextField::Description, None, &texts).unwrap();
        assert_eq!(preview.checked, preview.rows.len() + preview.same + preview.missing);
        assert_eq!(preview.limit, 512);        let rows: Vec<TextApply> = preview.rows.iter().filter(|row| row.problem.is_none()).take(3).map(|row| TextApply { index: row.index, current: row.current.clone(), text: row.text.clone() }).collect();
        if rows.is_empty() {
            return;
        }
        document.apply_texts(TextField::Description, &rows).unwrap();
        assert_eq!(document.item(rows[0].index).unwrap().description, rows[0].text);
        // A stale preview is refused; one undo restores every item.
        assert!(document.apply_texts(TextField::Description, &rows).is_err());
        document.undo().unwrap();
        assert_eq!(document.item(rows[0].index).unwrap().description, rows[0].current);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
