//! gshop.data: the item mall (gshop.data), bonus shop (gshop1.data) and cross-server shop (gshop2.data).
//!
//! Layout (`globaldata_load` in ZCommon/globaldataman.cpp, the server's template/globaldataman.cpp):
//! u32 timestamp, i32 count, `count` × `GSHOP_ITEM` (`#pragma pack(1)`, 2,630 bytes in the source; some
//! client builds append bytes: ForsakenJD 5, HDN/Reborn 30), then i32 main type count (≥ 7) and per main type
//! i32 id, `WORD[64]` name, i32 sub count, sub count × `WORD[64]`.
//!
//! The server keeps only the sale fields and checks a purchase by position: the item at the client's index
//! must have the same ID (`gplayer_imp::PlayerDoShopping`). The client refuses to open the mall when its
//! timestamp differs from the server's (`GShopVersionError`). So the server needs exactly the client's file;
//! the user copies it there after saving.

pub mod align;
pub mod compare;
pub mod layout;
pub mod texts;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

pub use compare::{ComparedShop, CopyReport, ExportCounts, ShopComparison};
pub use layout::Layout;
pub use texts::{TextApply, TextField, TextPreview};
use layout::text16;

const NAME_UNITS: usize = 64;
const MAX_ITEMS: usize = 65535;

/// A field value as the editor exchanges it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Text; for raw bytes, hex digits.
    Text(String),
}

/// A field the layout gives no meaning: shown and edited by its type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherField {
    /// Field name (inside groups `group[1].field`).
    pub path: String,
    /// `u8`, `u16`, `u32`, `i32`, `f32`, `bool`, `wstr:N`, `str:N` or `bytes:N`.
    pub ty: String,
    pub value: Value,
}

/// One shop item, as the editor shows it. Fields the file's layout does not have keep their defaults.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopItem {
    /// The item sold and how many.
    pub id: u32,
    pub num: u32,
    /// Shop icon (`Surfaces\QShop\…\*.dds`, GBK).
    pub icon: String,
    /// Price in mall cash and how long the item lasts (seconds, 0 = forever).
    pub price: u32,
    pub time: u32,
    /// Percentage charged while a discount scheme the item is in is active.
    pub discount: i32,
    /// Percentage of the price returned as bonus.
    pub bonus: i32,
    /// Bits 0–2 new / recommended / promotion; bits 16–23 sale schemes 1–8; bits 24–31 discount schemes 1–8.
    pub props: u32,
    /// Index of the main and sub category.
    pub main_type: i32,
    pub sub_type: i32,
    pub local_id: i32,
    pub description: String,
    pub name: String,
    pub has_present: bool,
    pub present_name: String,
    pub present_id: u32,
    pub present_count: u32,
    pub present_time: u32,
    pub present_icon: String,
    pub present_bind: bool,
    pub present_description: String,
    /// When it is on sale: 0 always, 1 a date range, 2 weekly, 3 monthly.
    pub valid_type: i32,
    pub valid_start: i32,
    pub valid_end: i32,
    /// Range: 1 start set, 2 end set (bits); weekly: Sunday–Saturday bits 0–6; monthly: days 1–31 bits 1–31.
    pub valid_param: i32,
    /// Comma-separated search keywords.
    pub search_keys: String,
    /// Lucky Bag shop: the item's position, and the item paid (with how many) instead of a price.
    #[serde(default)]
    pub place: i32,
    #[serde(default)]
    pub price_item: u32,
    #[serde(default)]
    pub price_item_count: u32,
    /// Fields of the layout without a meaning, in layout order.
    #[serde(default)]
    pub other: Vec<OtherField>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: i32,
    pub name: String,
    pub subs: Vec<String>,
}

/// A parsed shop file.
#[derive(Debug, Clone)]
pub struct ShopFile {
    pub timestamp: u32,
    /// Bytes per item; none when the file has no items.
    pub record_size: Option<usize>,
    /// Item records as stored.
    pub records: Vec<Vec<u8>>,
    pub categories: Vec<Category>,
    /// Stored name slots by their text (some files keep bytes after the terminator).
    pub raw_names: HashMap<String, Vec<u8>>,
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn i32_at(data: &[u8], at: usize) -> i32 {
    u32_at(data, at) as i32
}

/// The category block starting at `at`, when it ends exactly at the end of the file.
fn parse_categories(data: &[u8], mut at: usize, raw_names: &mut HashMap<String, Vec<u8>>) -> Option<Vec<Category>> {
    if at + 4 > data.len() {
        return None;
    }
    let count = i32_at(data, at);
    at += 4;
    if !(1..=1000).contains(&count) {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    let name = |data: &[u8], at: usize, raw_names: &mut HashMap<String, Vec<u8>>| {
        let slot = &data[at..at + NAME_UNITS * 2];
        let text = text16(slot);
        raw_names.entry(text.clone()).or_insert_with(|| slot.to_vec());
        text
    };
    for _ in 0..count {
        if at + 4 + NAME_UNITS * 2 + 4 > data.len() {
            return None;
        }
        let id = i32_at(data, at);
        let main = name(data, at + 4, raw_names);
        let subs = i32_at(data, at + 4 + NAME_UNITS * 2);
        at += 8 + NAME_UNITS * 2;
        if !(0..=1000).contains(&subs) || at + subs as usize * NAME_UNITS * 2 > data.len() {
            return None;
        }
        let subs: Vec<String> = (0..subs as usize).map(|index| name(data, at + index * NAME_UNITS * 2, raw_names)).collect();
        at += subs.len() * NAME_UNITS * 2;
        out.push(Category { id, name: main, subs });
    }
    (at == data.len()).then_some(out)
}

/// The file split at a record size (none: the category block does not end the file there).
fn split(data: &[u8], size: usize) -> Option<ShopFile> {
    let timestamp = u32_at(data, 0);
    let count = i32_at(data, 4) as usize;
    let at = count.checked_mul(size)?.checked_add(8)?;
    if at > data.len() {
        return None;
    }
    let mut raw_names = HashMap::new();
    let categories = parse_categories(data, at, &mut raw_names)?;
    let records = (0..count).map(|index| data[8 + index * size..8 + (index + 1) * size].to_vec()).collect();
    Some(ShopFile { timestamp, record_size: (count > 0).then_some(size), records, categories, raw_names })
}

/// What an unreadable file looks like, for creating a layout (sent after `NO_LAYOUT:` as JSON).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unknown {
    pub record_size: Option<usize>,
    pub items: usize,
    pub categories: usize,
}

/// Splits a shop file with a layout: the preferred one when its size fits, else the first that fits
/// (user layouts come first). Without one: `NO_LAYOUT:{record size, items, categories}`.
pub fn parse(data: &[u8], layouts: &[Layout], preferred: Option<&str>) -> Result<(ShopFile, Layout), String> {
    if data.len() < 12 {
        return Err("Too short for a gshop file".into());
    }
    let count = i32_at(data, 4);
    if !(0..=MAX_ITEMS as i32).contains(&count) {
        return Err(format!("Invalid item count {count}"));
    }
    let ordered: Vec<&Layout> = layouts.iter().filter(|layout| Some(layout.id.as_str()) == preferred).chain(layouts.iter().filter(|layout| Some(layout.id.as_str()) != preferred)).collect();
    for layout in &ordered {
        if let Some(file) = split(data, layout.size()) {
            return Ok((file, (*layout).clone()));
        }
    }
    // No layout fits: find the record size anyway, so a layout can be made for it.
    let found = (16..=16384).find_map(|size| split(data, size));
    let unknown = Unknown { record_size: found.as_ref().and_then(|file| file.record_size), items: count as usize, categories: found.as_ref().map_or(0, |file| file.categories.len()) };
    Err(format!("NO_LAYOUT:{}", serde_json::to_string(&unknown).unwrap_or_default()))
}

/// How a layout reads a file: the file's record size (found even when no layout fits), and every field of
/// the first items. For the layout editor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPreview {
    /// Bytes per item in the file (none: no items, or the category block was not found).
    pub record_size: Option<usize>,
    pub layout_size: usize,
    pub items: usize,
    /// Main category names, as a check that the file was split right.
    pub categories: Vec<String>,
    /// Per item shown: (field, type, value).
    pub rows: Vec<Vec<(String, String, String)>>,
}

pub fn preview(data: &[u8], layout: &Layout, rows: usize) -> Result<LayoutPreview, String> {
    if data.len() < 12 {
        return Err("Too short for a gshop file".into());
    }
    let file = split(data, layout.size()).or_else(|| (16..=16384).find_map(|size| split(data, size))).ok_or("The category list was not found at any record size; this is not a shop file of this kind")?;
    Ok(LayoutPreview {
        record_size: file.record_size,
        layout_size: layout.size(),
        items: file.records.len(),
        categories: file.categories.iter().map(|category| category.name.clone()).collect(),
        rows: file.records.iter().take(rows).map(|record| layout::describe(layout, record)).collect(),
    })
}

fn encode_name(text: &str, raw_names: &HashMap<String, Vec<u8>>, what: &str) -> Result<Vec<u8>, String> {
    if let Some(raw) = raw_names.get(text) {
        return Ok(raw.clone());
    }
    let mut slot = vec![0u8; NAME_UNITS * 2];
    layout::write(&layout::FieldType::Wstr { len: NAME_UNITS }, &mut slot, &Value::Text(text.to_string()), what)?;
    Ok(slot)
}

pub fn encode(timestamp: u32, records: &[Vec<u8>], categories: &[Category], raw_names: &HashMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    if records.len() > MAX_ITEMS {
        return Err(format!("At most {MAX_ITEMS} items (the server reads no more)"));
    }
    if categories.is_empty() {
        return Err("The shop needs at least one main category".into());
    }
    let mut out = Vec::with_capacity(16 + records.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(&timestamp.to_le_bytes());
    out.extend_from_slice(&(records.len() as i32).to_le_bytes());
    for record in records {
        out.extend_from_slice(record);
    }
    out.extend_from_slice(&(categories.len() as i32).to_le_bytes());
    for category in categories {
        out.extend_from_slice(&category.id.to_le_bytes());
        out.extend_from_slice(&encode_name(&category.name, raw_names, "A category name")?);
        out.extend_from_slice(&(category.subs.len() as i32).to_le_bytes());
        for sub in &category.subs {
            out.extend_from_slice(&encode_name(sub, raw_names, "A subcategory name")?);
        }
    }
    Ok(out)
}

// ── The open shop: one item list for the client and the server copy ──

/// Which shop a file is, by its name (gs.conf: MallData, BonusMallData, ZoneMallData).
pub fn shop_kind(path: &Path) -> &'static str {
    match path.file_name().and_then(|name| name.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("gshop.data") => "Item mall",
        Some("gshop1.data") => "Bonus shop",
        Some("gshop2.data") => "Cross-server shop",
        Some("gshop4.data") => "Lucky bag shop",
        _ => "Shop",
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    item: ShopItem,
    /// The stored record, kept where fields do not change.
    raw: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
enum Change {
    Item { index: usize, before: Option<Entry>, after: Option<Entry> },
    Categories { before: Vec<Category>, after: Vec<Category> },
}

struct JournalEntry {
    id: u64,
    label: String,
    time: i64,
    changes: Vec<Change>,
}

struct Copy {
    path: PathBuf,
    record_size: Option<usize>,
    timestamp: u32,
    disk: String,
    backed_up: bool,
}

fn digest(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub index: usize,
    pub id: u32,
    pub num: u32,
    pub name: String,
    pub icon: String,
    pub price: u32,
    pub time: u32,
    pub discount: i32,
    pub props: u32,
    pub main_type: i32,
    pub sub_type: i32,
    pub has_present: bool,
    pub valid_type: i32,
    pub place: i32,
    pub price_item: u32,
    pub price_item_count: u32,
    pub changed: bool,
}

/// A layout as the UI lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutInfo {
    pub id: String,
    pub name: String,
    pub size: usize,
    pub builtin: bool,
}

impl From<&Layout> for LayoutInfo {
    fn from(layout: &Layout) -> Self {
        LayoutInfo { id: layout.id.clone(), name: layout.name.clone(), size: layout.size(), builtin: layout.builtin }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub id: u64,
    pub label: String,
    pub time: i64,
    pub undone: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub path: String,
    pub kind: &'static str,
    pub timestamp: u32,
    pub record_size: Option<usize>,
    /// The layout the items are read with, the other layouts of the same size, and the meanings it has.
    pub layout: LayoutInfo,
    pub alternatives: Vec<LayoutInfo>,
    pub meanings: Vec<String>,
    /// Text meanings with their slot size and whether it is UTF-16 (characters) or GBK (bytes).
    pub text_limits: HashMap<String, (usize, bool)>,
    pub items: Vec<Summary>,
    pub categories: Vec<Category>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub history: Vec<HistoryRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub backup: Option<String>,
    pub timestamp: u32,
}

/// A category change; items keep pointing at the same subcategory.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CategoryOp {
    RenameMain { main: usize, name: String },
    AddSub { main: usize, name: String },
    RenameSub { main: usize, sub: usize, name: String },
    /// Items in it move to `move_to` (an index before the removal) or block the removal.
    RemoveSub {
        main: usize,
        sub: usize,
        #[serde(rename = "moveTo")]
        move_to: Option<usize>,
    },
    MoveSub { main: usize, sub: usize, to: usize },
}

pub struct Document {
    file: Copy,
    layout: Layout,
    alternatives: Vec<LayoutInfo>,
    entries: Vec<Entry>,
    categories: Vec<Category>,
    raw_names: HashMap<String, Vec<u8>>,
    /// Items as opened or last saved, to mark changed ones.
    saved: Vec<ShopItem>,
    done: Vec<JournalEntry>,
    undone: Vec<JournalEntry>,
    next_entry: u64,
    saved_entries: Option<usize>,
}

fn read_copy(path: &Path, layouts: &[Layout], preferred: Option<&str>) -> Result<(ShopFile, Layout, Copy), String> {
    let data = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let (file, layout) = parse(&data, layouts, preferred)?;
    let check = encode(file.timestamp, &file.records, &file.categories, &file.raw_names)?;
    if check != data {
        return Err(format!("{} does not write back byte for byte; it is not supported", path.display()));
    }
    let copy = Copy { path: path.to_path_buf(), record_size: file.record_size, timestamp: file.timestamp, disk: digest(&data), backed_up: false };
    Ok((file, layout, copy))
}

impl Document {
    /// Opens a shop file with the preferred layout when it fits, else the first layout that does.
    pub fn open(path: impl AsRef<Path>, layouts: &[Layout], preferred: Option<&str>) -> Result<Self, String> {
        let (file, layout, copy) = read_copy(path.as_ref(), layouts, preferred)?;
        let entries: Vec<Entry> = file.records.iter().map(|record| Entry { item: layout::decode(&layout, record), raw: Some(record.clone()) }).collect();
        let saved = entries.iter().map(|entry| entry.item.clone()).collect();
        let size = layout.size();
        let alternatives = layouts.iter().filter(|other| other.size() == size && other.id != layout.id).map(LayoutInfo::from).collect();
        Ok(Document { file: copy, layout, alternatives, entries, categories: file.categories, raw_names: file.raw_names, saved, done: Vec::new(), undone: Vec::new(), next_entry: 1, saved_entries: Some(0) })
    }

    pub fn path(&self) -> &Path {
        &self.file.path
    }


    pub fn item(&self, index: usize) -> Result<ShopItem, String> {
        self.entries.get(index).map(|entry| entry.item.clone()).ok_or_else(|| format!("No item {}", index + 1))
    }

    pub fn view(&self) -> View {
        let items = self.entries.iter().enumerate().map(|(index, entry)| {
            let item = &entry.item;
            Summary {
                index,
                id: item.id,
                num: item.num,
                name: item.name.clone(),
                icon: item.icon.clone(),
                price: item.price,
                time: item.time,
                discount: item.discount,
                props: item.props,
                main_type: item.main_type,
                sub_type: item.sub_type,
                has_present: item.has_present,
                valid_type: item.valid_type,
                place: item.place,
                price_item: item.price_item,
                price_item_count: item.price_item_count,
                changed: self.saved.get(index) != Some(item),
            }
        }).collect();
        let row = |entry: &JournalEntry, undone: bool| HistoryRow { id: entry.id, label: entry.label.clone(), time: entry.time, undone };
        let mut history: Vec<HistoryRow> = self.done.iter().map(|entry| row(entry, false)).collect();
        history.extend(self.undone.iter().rev().map(|entry| row(entry, true)));
        View {
            path: self.file.path.display().to_string(),
            kind: shop_kind(&self.file.path),
            timestamp: self.file.timestamp,
            record_size: self.file.record_size,
            layout: LayoutInfo::from(&self.layout),
            alternatives: self.alternatives.clone(),
            meanings: self.layout.meanings(),
            text_limits: self.layout.text_limits(),
            items,
            categories: self.categories.clone(),
            dirty: self.saved_entries != Some(self.done.len()),
            can_undo: !self.done.is_empty(),
            can_redo: !self.undone.is_empty(),
            history,
        }
    }

    fn apply(&mut self, change: &Change, forward: bool) {
        match change {
            Change::Item { index, before, after } => {
                let (from, to) = if forward { (before, after) } else { (after, before) };
                match (from, to) {
                    (Some(_), Some(entry)) => self.entries[*index] = entry.clone(),
                    (None, Some(entry)) => self.entries.insert(*index, entry.clone()),
                    (Some(_), None) => {
                        self.entries.remove(*index);
                    }
                    (None, None) => {}
                }
            }
            Change::Categories { before, after } => self.categories = if forward { after.clone() } else { before.clone() },
        }
    }

    /// The file must still write (text lengths, category count).
    fn check(&self) -> Result<(), String> {
        self.records()?;
        encode(0, &[], &self.categories, &self.raw_names).map(|_| ())
    }

    fn record_all(&mut self, label: String, changes: Vec<Change>) -> Result<View, String> {
        for change in &changes {
            self.apply(change, true);
        }
        if let Err(error) = self.check() {
            for change in changes.iter().rev() {
                self.apply(change, false);
            }
            return Err(error);
        }
        if self.saved_entries.is_some_and(|saved| saved > self.done.len()) {
            self.saved_entries = None;
        }
        self.undone.clear();
        self.done.push(JournalEntry { id: self.next_entry, label, time: Local::now().timestamp(), changes });
        self.next_entry += 1;
        Ok(self.view())
    }

    pub fn set_item(&mut self, index: usize, item: ShopItem, label: &str) -> Result<View, String> {
        let before = self.entries.get(index).cloned().ok_or_else(|| format!("No item {}", index + 1))?;
        if before.item == item {
            return Ok(self.view());
        }
        let after = Entry { item, ..before.clone() };
        self.record_all(label.to_string(), vec![Change::Item { index, before: Some(before), after: Some(after) }])
    }

    /// Copies an item below itself (new items are made by cloning).
    pub fn clone_item(&mut self, index: usize) -> Result<(View, usize), String> {
        let entry = self.entries.get(index).cloned().ok_or_else(|| format!("No item {}", index + 1))?;
        if self.entries.len() >= MAX_ITEMS {
            return Err(format!("At most {MAX_ITEMS} items"));
        }
        let view = self.record_all(format!("Clone item {}", index + 1), vec![Change::Item { index: index + 1, before: None, after: Some(entry) }])?;
        Ok((view, index + 1))
    }

    pub fn delete_item(&mut self, index: usize) -> Result<View, String> {
        let entry = self.entries.get(index).cloned().ok_or_else(|| format!("No item {}", index + 1))?;
        self.record_all(format!("Delete item {}", index + 1), vec![Change::Item { index, before: Some(entry), after: None }])
    }

    /// Moves an item to another position (the order is what purchases go by).
    pub fn move_item(&mut self, index: usize, to: usize) -> Result<View, String> {
        let entry = self.entries.get(index).cloned().ok_or_else(|| format!("No item {}", index + 1))?;
        if to >= self.entries.len() {
            return Err(format!("No position {}", to + 1));
        }
        if to == index {
            return Ok(self.view());
        }
        let label = format!("Move item {} to {}", index + 1, to + 1);
        self.record_all(label, vec![Change::Item { index, before: Some(entry.clone()), after: None }, Change::Item { index: to, before: None, after: Some(entry) }])
    }

    /// A category change, with the items of the main category renumbered to keep their subcategory.
    pub fn edit_categories(&mut self, op: CategoryOp) -> Result<View, String> {
        let before = self.categories.clone();
        let mut after = before.clone();
        let check_main = |main: usize| if main < before.len() { Ok(()) } else { Err(format!("No main category {}", main + 1)) };
        // Old subcategory index → new one (none: removed) for one main category.
        let mut remap: Option<(usize, Vec<Option<usize>>)> = None;
        let label = match &op {
            CategoryOp::RenameMain { main, name } => {
                check_main(*main)?;
                after[*main].name = name.clone();
                format!("Rename category {}", main + 1)
            }
            CategoryOp::AddSub { main, name } => {
                check_main(*main)?;
                after[*main].subs.push(name.clone());
                format!("Add a subcategory to {}", before[*main].name)
            }
            CategoryOp::RenameSub { main, sub, name } => {
                check_main(*main)?;
                *after[*main].subs.get_mut(*sub).ok_or("No such subcategory")? = name.clone();
                format!("Rename subcategory {}", before[*main].subs[*sub])
            }
            CategoryOp::RemoveSub { main, sub, move_to } => {
                check_main(*main)?;
                let count = before[*main].subs.len();
                if *sub >= count {
                    return Err("No such subcategory".into());
                }
                let used = self.entries.iter().filter(|entry| entry.item.main_type == *main as i32 && entry.item.sub_type == *sub as i32).count();
                let target = match move_to {
                    Some(target) if *target == *sub || *target >= count => return Err("Move its items to another subcategory".into()),
                    Some(target) => Some(*target),
                    None if used > 0 => return Err(format!("{used} item{} use this subcategory; choose where they go", if used == 1 { "" } else { "s" })),
                    None => None,
                };
                after[*main].subs.remove(*sub);
                let shift = |old: usize| if old > *sub { old - 1 } else { old };
                remap = Some((*main, (0..count).map(|old| if old == *sub { target.map(shift) } else { Some(shift(old)) }).collect()));
                format!("Remove subcategory {}", before[*main].subs[*sub])
            }
            CategoryOp::MoveSub { main, sub, to } => {
                check_main(*main)?;
                let count = before[*main].subs.len();
                if *sub >= count || *to >= count {
                    return Err("No such subcategory".into());
                }
                let name = after[*main].subs.remove(*sub);
                after[*main].subs.insert(*to, name);
                let order: Vec<usize> = {
                    let mut order: Vec<usize> = (0..count).collect();
                    let moved = order.remove(*sub);
                    order.insert(*to, moved);
                    order
                };
                let mut map = vec![None; count];
                for (new, old) in order.into_iter().enumerate() {
                    map[old] = Some(new);
                }
                remap = Some((*main, map));
                format!("Move subcategory {}", before[*main].subs[*sub])
            }
        };
        let mut changes = vec![Change::Categories { before, after }];
        if let Some((main, map)) = remap {
            for (index, entry) in self.entries.iter().enumerate() {
                if entry.item.main_type != main as i32 {
                    continue;
                }
                let Some(Some(new)) = usize::try_from(entry.item.sub_type).ok().and_then(|old| map.get(old)) else { continue };
                if *new as i32 != entry.item.sub_type {
                    let mut moved = entry.clone();
                    moved.item.sub_type = *new as i32;
                    changes.push(Change::Item { index, before: Some(entry.clone()), after: Some(moved) });
                }
            }
        }
        self.record_all(label, changes)
    }

    pub fn undo(&mut self) -> Result<View, String> {
        let entry = self.done.pop().ok_or("Nothing to undo")?;
        for change in entry.changes.iter().rev() {
            self.apply(change, false);
        }
        self.undone.push(entry);
        Ok(self.view())
    }

    pub fn redo(&mut self) -> Result<View, String> {
        let entry = self.undone.pop().ok_or("Nothing to redo")?;
        for change in &entry.changes {
            self.apply(change, true);
        }
        self.done.push(entry);
        Ok(self.view())
    }

    /// The records to write: each item over its stored record.
    fn records(&self) -> Result<Vec<Vec<u8>>, String> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| layout::encode(&self.layout, &entry.item, entry.raw.as_deref()).map_err(|error| format!("Item {}: {error}", index + 1)))
            .collect()
    }

    /// Writes the file with a new timestamp (copy it to the server afterwards: it needs the same file).
    pub fn save(&mut self, backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
        let records = self.records()?;
        let copy = &mut self.file;
        if !replace_changed {
            if let Ok(data) = std::fs::read(&copy.path) {
                if digest(&data) != copy.disk {
                    return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", copy.path.display()));
                }
            }
        }
        let timestamp = (Local::now().timestamp().max(0) as u32).max(copy.timestamp.saturating_add(1));
        let data = encode(timestamp, &records, &self.categories, &self.raw_names)?;
        let back = split(&data, self.layout.size()).ok_or("The written shop does not read back; nothing was saved")?;
        if back.records.len() != self.entries.len() || back.timestamp != timestamp {
            return Err("The written shop does not read back as expected; nothing was saved".into());
        }
        let mut report = SaveReport { path: copy.path.display().to_string(), backup: None, timestamp };
        if backup && !copy.backed_up && copy.path.exists() {
            let archive = crate::backup::archive(&copy.path, std::slice::from_ref(&copy.path))?;
            report.backup = Some(archive.display().to_string());
            copy.backed_up = true;
        }
        let temporary = copy.path.with_extension("data.jdide-saving");
        std::fs::write(&temporary, &data).map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
        if let Ok(meta) = std::fs::metadata(&copy.path) {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(&copy.path, permissions);
        }
        std::fs::rename(&temporary, &copy.path).map_err(|error| format!("Could not replace {}: {error}", copy.path.display()))?;
        copy.disk = digest(&data);
        copy.timestamp = timestamp;
        // The stored bytes are now the written ones.
        for (entry, record) in self.entries.iter_mut().zip(back.records) {
            entry.raw = Some(record);
        }
        self.saved = self.entries.iter().map(|entry| entry.item.clone()).collect();
        self.saved_entries = Some(self.done.len());
        Ok(report)
    }
}

// ── Problems ──

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// "error" (the item cannot be bought or shown) or "warning".
    pub severity: &'static str,
    /// The item position, or none for the whole shop.
    pub index: Option<usize>,
    pub message: String,
}

impl Document {
    /// Item and gift templates the shop uses.
    pub fn template_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.entries.iter().flat_map(|entry| [entry.item.id, if entry.item.has_present { entry.item.present_id } else { 0 }, entry.item.price_item]).filter(|id| *id != 0).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// `known`: which template IDs the open elements.data has (none: no elements.data open).
    pub fn problems(&self, known: Option<&std::collections::HashSet<u32>>) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut add = |severity, index, message: String| out.push(Problem { severity, index, message });
        // Only what this file's layout stores is checked.
        let meanings = self.layout.meanings();
        let has = |meaning: &str| meanings.iter().any(|known| known == meaning);
        let mut first_of: HashMap<(u32, u32), usize> = HashMap::new();
        // Old shops use no sale schemes at all; their servers predate the check, so say it once.
        let schemes = self.entries.iter().any(|entry| entry.item.props & 0x00FF_0000 != 0);
        if has("props") && !schemes && !self.entries.is_empty() {
            add("warning", None, "No item is in a sale scheme. Old servers do not use them, but a server with mall sale schemes refuses to sell every item".into());
        }
        for (index, entry) in self.entries.iter().enumerate() {
            let item = &entry.item;
            let at = Some(index);
            if has("id") {
                if item.id == 0 {
                    add("error", at, "No item to sell (ID 0)".into());
                } else if known.is_some_and(|known| !known.contains(&item.id)) {
                    add("error", at, format!("Item {} is not in the open elements.data", item.id));
                }
            }
            if has("num") && item.num == 0 {
                add("warning", at, "Sells 0 of the item".into());
            }
            if has("price") && item.price == 0 {
                add("error", at, "Price 0: the server refuses to sell it".into());
            }
            if has("props") {
                if schemes && item.props & 0x00FF_0000 == 0 {
                    add("error", at, "In no sale scheme: the server refuses to sell it".into());
                }
                if has("discount") && item.props & 0xFF00_0000 != 0 && !(1..=100).contains(&item.discount) {
                    add("warning", at, format!("Discount {}% while in a discount scheme (the price becomes {} × {} / 100)", item.discount, item.price, item.discount));
                }
            }
            if has("main_type") {
                match self.categories.get(item.main_type.max(0) as usize).filter(|_| item.main_type >= 0) {
                    None => add("error", at, format!("Main category {} does not exist: the item is not shown", item.main_type)),
                    Some(category) if has("sub_type") && (item.sub_type < 0 || item.sub_type as usize >= category.subs.len()) => add("error", at, format!("{} has no subcategory {}: the item is not shown", category.name, item.sub_type + 1)),
                    _ => {}
                }
            }
            if has("price_item") {
                if item.price_item == 0 {
                    add("error", at, "No item to pay with (price item 0)".into());
                } else if known.is_some_and(|known| !known.contains(&item.price_item)) {
                    add("error", at, format!("Price item {} is not in the open elements.data", item.price_item));
                }
                if has("price_item_count") && item.price_item_count == 0 {
                    add("warning", at, "Costs 0 of the price item".into());
                }
            }
            if has("has_present") && item.has_present {
                if has("present_id") && item.present_id == 0 {
                    add("error", at, "Has a gift without an item".into());
                } else if has("present_id") && known.is_some_and(|known| !known.contains(&item.present_id)) {
                    add("error", at, format!("Gift {} is not in the open elements.data", item.present_id));
                }
                if has("present_count") && item.present_count == 0 {
                    add("warning", at, "The gift count is 0".into());
                }
            }
            if has("valid_type") {
                if item.valid_type == 1 && item.valid_param & 3 == 3 && item.valid_end <= item.valid_start {
                    add("warning", at, "The sale ends before it starts".into());
                }
                if !(0..=3).contains(&item.valid_type) {
                    add("warning", at, format!("Unknown sale window type {}", item.valid_type));
                }
            }
            if has("name") && item.name.trim().is_empty() {
                add("warning", at, "No name".into());
            }
            if has("id") {
                if let Some(first) = first_of.insert((item.id, item.num), index) {
                    first_of.insert((item.id, item.num), first);
                    add("warning", at, format!("Item {} × {} is also item {}; the client finds the first by ID", item.id, item.num, first + 1));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::layout::{Field, FieldType};
    use super::*;

    fn samples() -> Vec<PathBuf> {
        let roots = ["E:/Games/ForsakenJD/element/data", "E:/Games/Elite Jade Dynasty - HDN/element/data", "E:/Games/Jade Dynasty Reborn/element/data", "E:/Games/XtremeJade/element/data", "E:/Game Dev/JD/1559/gamed/config"];
        roots.iter().flat_map(|root| ["gshop.data", "gshop1.data", "gshop2.data", "gshop3.data", "gshop4.data"].map(|name| Path::new(root).join(name))).filter(|path| path.is_file()).collect()
    }

    #[test]
    fn built_in_layouts_are_valid_and_sized() {
        let layouts = layout::builtin();
        let sizes: Vec<(String, usize)> = layouts.iter().map(|layout| (layout.id.clone(), layout.size())).collect();
        assert_eq!(sizes, vec![("source".to_string(), 2630), ("forsakenjd".to_string(), 2635), ("hdn".to_string(), 2660), ("luckybag".to_string(), 357)]);
        for layout in &layouts {
            layout.validate().unwrap();
            let text = serde_json::to_string(layout).unwrap();
            let back: Layout = serde_json::from_str(&text).unwrap();
            assert_eq!(back.fields, layout.fields);
        }
    }

    #[test]
    fn real_shops_read_and_write_byte_for_byte() {
        let layouts = layout::builtin();
        for path in samples() {
            let data = std::fs::read(&path).unwrap();
            let (file, layout) = parse(&data, &layouts, None).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            // The Lucky Bag shop has 3 main categories (New, Packs, Other); the others 7 or more.
            assert!(file.categories.len() >= if file.record_size == Some(357) { 3 } else { 7 }, "{}", path.display());
            let expected = match file.record_size {
                None => "source",
                Some(2630) => "source",
                Some(2635) => "forsakenjd",
                Some(2660) => "hdn",
                Some(357) => "luckybag",
                other => panic!("{} {other:?}", path.display()),
            };
            assert_eq!(layout.id, expected, "{}", path.display());
            for record in &file.records {
                let item = layout::decode(&layout, record);
                assert_eq!(&layout::encode(&layout, &item, Some(record)).unwrap(), record);
                // Rebuilt from the item alone (no stored bytes), every field the layout reads comes back.
                let rebuilt = layout::encode(&layout, &item, None).unwrap();
                assert_eq!(layout::decode(&layout, &rebuilt), item);
            }
            assert_eq!(encode(file.timestamp, &file.records, &file.categories, &file.raw_names).unwrap(), data, "{}", path.display());
        }
    }

    #[test]
    fn shop_icons_come_from_the_client_packages() {
        let client = Path::new("E:/Games/ForsakenJD");
        let Ok(data) = std::fs::read(client.join("element/data/gshop.data")) else { return };
        let (file, layout) = parse(&data, &layout::builtin(), None).unwrap();
        let res = crate::client::Resources::new(crate::client::inspect(client).unwrap());
        let icons: Vec<String> = file.records.iter().map(|record| layout::decode(&layout, record).icon).filter(|icon| !icon.is_empty()).collect();
        let found = icons.iter().filter(|icon| res.image_at(icon).is_ok()).count();
        assert!(found * 10 >= icons.len() * 9, "{found} of {} icons", icons.len());
        assert_eq!(&res.image_at(&icons[0]).unwrap().bytes[1..4], b"PNG");
        // The icon picker lists the shop folder.
        let listed = res.images_in("surfaces/qshop").unwrap();
        assert!(listed.len() >= 100, "{}", listed.len());
        assert!(listed.iter().any(|path| path.eq_ignore_ascii_case(&icons[0])), "{}", icons[0]);
    }

    #[test]
    fn a_user_layout_reads_a_newer_record() {
        let Ok(data) = std::fs::read("E:/Game Dev/JD/1559/gamed/config/gshop.data") else { return };
        let (file, source) = parse(&data, &layout::builtin(), None).unwrap();
        // A "newer" shop: a second price (u32) inserted after the price in every record.
        let mut newer = data[..8].to_vec();
        for (index, record) in file.records.iter().enumerate() {
            newer.extend_from_slice(&record[..140]);
            newer.extend_from_slice(&(7000 + index as u32).to_le_bytes());
            newer.extend_from_slice(&record[140..]);
        }
        newer.extend_from_slice(&data[8 + file.records.len() * 2630..]);
        let error = parse(&newer, &layout::builtin(), None).unwrap_err();
        assert!(error.starts_with("NO_LAYOUT:") && error.contains("\"recordSize\":2634"), "{error}");

        let mut fields = source.fields.clone();
        let at = fields.iter().position(|field| field.name == "price").unwrap() + 1;
        fields.insert(at, Field { name: "second_price".into(), ty: FieldType::U32, meaning: None, note: String::new() });
        let user = Layout { id: "test-newer".into(), name: "Newer".into(), description: String::new(), fields, builtin: false };
        user.validate().unwrap();
        let config = std::env::temp_dir().join(format!("jdide-gshop-layouts-{}", std::process::id()));
        layout::save(&config, &user).unwrap();
        let layouts = layout::all(Some(&config));
        assert_eq!(layouts[0].id, "test-newer");
        let (read, chosen) = parse(&newer, &layouts, None).unwrap();
        assert_eq!(chosen.id, "test-newer");
        let first = layout::decode(&chosen, &read.records[0]);
        let original = layout::decode(&source, &file.records[0]);
        assert_eq!((first.id, first.price, &first.name), (original.id, original.price, &original.name));
        assert_eq!(first.other, vec![OtherField { path: "second_price".into(), ty: "u32".into(), value: Value::Int(7000) }]);
        assert_eq!(encode(read.timestamp, &read.records, &read.categories, &read.raw_names).unwrap(), newer);
        // Editing the extra field writes it.
        let mut changed = first.clone();
        changed.other[0].value = Value::Int(1234);
        let record = layout::encode(&chosen, &changed, Some(&read.records[0])).unwrap();
        assert_eq!(u32::from_le_bytes(record[140..144].try_into().unwrap()), 1234);
        layout::delete(&config, "test-newer").unwrap();
        let _ = std::fs::remove_dir_all(&config);

        // Mistakes are refused.
        let mut wrong = user.clone();
        wrong.fields[0].ty = FieldType::Wstr { len: 4 };
        assert!(wrong.validate().unwrap_err().contains("integer"));
        let mut twice = user.clone();
        twice.fields[1].meaning = Some("id".into());
        assert!(twice.validate().unwrap_err().contains("Two fields have the meaning id"));
        assert!(layout::save(&config, &Layout { id: "source".into(), ..user }).is_err(), "built-in IDs are kept");
    }

    #[test]
    fn edits_save_and_reopen() {
        let source = Path::new("E:/Game Dev/JD/1559/gamed/config/gshop.data");
        if !source.is_file() {
            return;
        }
        let layouts = layout::builtin();
        let folder = std::env::temp_dir().join(format!("jdide-gshop-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("gshop.data");
        std::fs::copy(source, &path).unwrap();
        let mut document = Document::open(&path, &layouts, None).unwrap();
        let view = document.view();
        assert_eq!((view.kind, view.layout.id.as_str()), ("Item mall", "source"));

        // Edit, clone, move, category changes; one undo step each.
        let mut item = document.item(1).unwrap();
        item.price = 4321;
        item.name = "Edited".into();
        item.description = "Line one\\rLine two".into();
        document.set_item(1, item, "Edit price").unwrap();
        let (_, clone) = document.clone_item(1).unwrap();
        assert_eq!(clone, 2);
        document.move_item(2, 0).unwrap();
        assert_eq!(document.item(0).unwrap().price, 4321);
        let main = document.entries[3].item.main_type as usize;
        let sub = document.entries[3].item.sub_type as usize;
        let subs = document.categories[main].subs.len();
        document.edit_categories(CategoryOp::AddSub { main, name: "New sub".into() }).unwrap();
        document.edit_categories(CategoryOp::MoveSub { main, sub, to: subs }).unwrap();
        assert_eq!(document.entries[3].item.sub_type as usize, subs, "items follow their subcategory");
        assert!(document.edit_categories(CategoryOp::RemoveSub { main, sub: subs, move_to: None }).is_err());
        document.undo().unwrap();
        assert_eq!(document.entries[3].item.sub_type as usize, sub);
        document.redo().unwrap();
        let mut long = document.item(0).unwrap();
        long.name = "x".repeat(40);
        assert!(document.set_item(0, long, "Too long").is_err(), "names over 31 characters are refused");

        let old = view.timestamp;
        let report = document.save(true, false).unwrap();
        assert!(report.timestamp > old && report.backup.is_some());
        let reopened = Document::open(&path, &layouts, None).unwrap();
        assert_eq!(reopened.item(0).unwrap().name, "Edited");
        assert_eq!(reopened.item(0).unwrap().description, "Line one\\rLine two");
        assert_eq!(reopened.view().items.len(), view.items.len() + 1);
        // Items that did not change keep their stored bytes (the moved subcategory renumbers its own).
        let (before, _) = parse(&std::fs::read(source).unwrap(), &layouts, None).unwrap();
        let (after, _) = parse(&std::fs::read(&path).unwrap(), &layouts, None).unwrap();
        let base = &layouts[0];
        let untouched = (3..before.records.len()).filter(|&index| layout::decode(base, &before.records[index]).main_type as usize != main).collect::<Vec<_>>();
        assert!(untouched.len() > 100);
        assert!(untouched.iter().all(|&index| after.records[index + 1] == before.records[index]));
        let _ = std::fs::remove_dir_all(&folder);
    }
}


