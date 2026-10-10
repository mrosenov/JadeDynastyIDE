//! Comparing the open shop with another shop file or a JSON export, copying items across, and the JSON
//! export itself.
//!
//! Items are paired by what they sell (item ID and count); repeated pairs are matched in file order.
//! Categories are matched by name (main categories fall back to the same position), so shops whose
//! categories are numbered differently still compare. Only the fields both layouts read are compared and
//! copied; fields without a meaning are matched by name and type.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::layout::{self, Layout};
use super::{parse, shop_kind, Category, Change, Document, Entry, ShopItem, View, MAX_ITEMS};

const FORMAT: &str = "jdide-gshop";

/// A shop JSON export (`jdide-gshop` version 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopJson {
    pub format: String,
    pub version: u32,
    /// Item mall, bonus shop or cross-server shop.
    pub kind: String,
    pub timestamp: u32,
    /// The layout the items were read with and its meanings (the item fields the file really has).
    pub layout: String,
    pub meanings: Vec<String>,
    /// All categories (items refer to them by position).
    pub categories: Vec<Category>,
    pub items: Vec<ShopItem>,
}

/// The other shop of a comparison (read only).
pub struct ComparedShop {
    pub path: PathBuf,
    pub json: bool,
    pub kind: String,
    pub layout: String,
    pub meanings: Vec<String>,
    pub categories: Vec<Category>,
    pub items: Vec<ShopItem>,
}

impl ComparedShop {
    /// Opens a shop file (read with the first layout that fits) or a JSON export.
    pub fn open(path: impl AsRef<Path>, layouts: &[Layout]) -> Result<Self, String> {
        let path = path.as_ref();
        let data = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("json")) {
            let export: ShopJson = serde_json::from_slice(&data).map_err(|error| format!("{} is not a shop JSON export: {error}", path.display()))?;
            if export.format != FORMAT {
                return Err(format!("{} is not a shop JSON export", path.display()));
            }
            if export.version != 1 {
                return Err(format!("{} was written by a newer JD IDE (version {})", path.display(), export.version));
            }
            if export.categories.is_empty() {
                return Err(format!("{} has no categories", path.display()));
            }
            return Ok(ComparedShop { path: path.to_path_buf(), json: true, kind: export.kind, layout: export.layout, meanings: export.meanings, categories: export.categories, items: export.items });
        }
        let (file, layout) = parse(&data, layouts, None).map_err(|error| if error.starts_with("NO_LAYOUT:") { format!("No item layout reads {}; open it in the editor first to create one", path.display()) } else { error })?;
        let items = file.records.iter().map(|record| layout::decode(&layout, record)).collect();
        Ok(ComparedShop { path: path.to_path_buf(), json: false, kind: shop_kind(path).to_string(), layout: layout.name.clone(), meanings: layout.meanings(), categories: file.categories, items })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareRow {
    /// `missing` (only in the other shop), `different` or `only_here`.
    pub status: &'static str,
    pub here: Option<usize>,
    pub there: Option<usize>,
    pub id: u32,
    pub num: u32,
    pub name: String,
    pub price: u32,
    /// "Main › Sub" (in the other shop for missing and different items).
    pub category: String,
    /// Differing fields: meanings (`price`, `name`, …), `category`, or other field paths.
    pub fields: Vec<String>,
    /// Why the item cannot be copied.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShopComparison {
    pub path: String,
    pub json: bool,
    pub kind: String,
    pub layout: String,
    /// Items in the other shop, and how many are the same here.
    pub items: usize,
    pub same: usize,
    /// Meanings only one side has (not compared).
    pub only_there: Vec<String>,
    pub only_here: Vec<String>,
    pub rows: Vec<CompareRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyReport {
    pub replaced: usize,
    pub added: usize,
    /// Subcategories that were added ("Main › Sub").
    pub subcategories: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportCounts {
    pub items: usize,
    pub categories: usize,
}

/// Where an item of the other shop belongs here.
enum Place {
    At(usize, i32),
    /// A subcategory this shop does not have (added on copy).
    NewSub(usize, String),
    Blocked(String),
}

fn place(there: &[Category], here: &[Category], main: i32, sub: i32) -> Place {
    let Some(category) = usize::try_from(main).ok().and_then(|main| there.get(main)) else {
        return Place::Blocked(format!("Its main category {} does not exist in the other shop", main + 1));
    };
    let name = category.name.trim();
    let Some(mapped) = here.iter().position(|entry| entry.name.trim() == name).or_else(|| ((main as usize) < here.len()).then_some(main as usize)) else {
        return Place::Blocked(format!("No main category {name} here"));
    };
    match usize::try_from(sub).ok().and_then(|sub| category.subs.get(sub)) {
        // An invalid subcategory stays as it is.
        None => Place::At(mapped, sub),
        Some(sub_name) => match here[mapped].subs.iter().position(|entry| entry.trim() == sub_name.trim()) {
            Some(index) => Place::At(mapped, index as i32),
            None => Place::NewSub(mapped, sub_name.clone()),
        },
    }
}

fn category_name(categories: &[Category], main: i32, sub: i32) -> String {
    let Some(category) = usize::try_from(main).ok().and_then(|main| categories.get(main)) else { return format!("({})", main + 1) };
    match usize::try_from(sub).ok().and_then(|sub| category.subs.get(sub)) {
        Some(sub) => format!("{} › {sub}", category.name),
        None => category.name.clone(),
    }
}

/// For each item there, the item here it pairs with: same ID and count; identical items first (`same`, so a
/// removed repeat does not shift the pairs), then the rest in order.
fn pair(here: &[ShopItem], there: &[ShopItem], same: impl Fn(usize, usize) -> bool) -> Vec<Option<usize>> {
    let mut queues: HashMap<(u32, u32), VecDeque<usize>> = HashMap::new();
    for (index, item) in here.iter().enumerate() {
        queues.entry((item.id, item.num)).or_default().push_back(index);
    }
    let mut out = vec![None; there.len()];
    for (index, item) in there.iter().enumerate() {
        let Some(queue) = queues.get_mut(&(item.id, item.num)) else { continue };
        if let Some(at) = queue.iter().position(|&candidate| same(candidate, index)) {
            out[index] = queue.remove(at);
        }
    }
    for (index, item) in there.iter().enumerate() {
        if out[index].is_none() {
            out[index] = queues.get_mut(&(item.id, item.num)).and_then(VecDeque::pop_front);
        }
    }
    out
}

/// `item` with the compared fields of `from`: shared meanings, the category at `main`/`sub`, and fields
/// without a meaning that have the same name and type.
fn merged(item: &ShopItem, from: &ShopItem, shared: &[String], main: usize, sub: i32) -> ShopItem {
    let mut out = item.clone();
    for meaning in shared {
        if meaning != "main_type" && meaning != "sub_type" {
            layout::set(&mut out, meaning, layout::get(from, meaning));
        }
    }
    out.main_type = main as i32;
    out.sub_type = sub;
    for field in &mut out.other {
        if let Some(source) = from.other.iter().find(|source| source.path == field.path && source.ty == field.ty) {
            field.value = source.value.clone();
        }
    }
    out
}

impl Document {
    fn shared_meanings(&self, other: &ComparedShop) -> Vec<String> {
        self.layout.meanings().into_iter().filter(|meaning| other.meanings.contains(meaning)).collect()
    }

    fn pairs(&self, here: &[ShopItem], other: &ComparedShop, shared: &[String]) -> Vec<Option<usize>> {
        pair(here, &other.items, |mine, theirs| self.differing(&here[mine], &other.items[theirs], other, shared).is_empty())
    }

    /// Fields that differ between an item here and one there.
    fn differing(&self, here: &ShopItem, there: &ShopItem, other: &ComparedShop, shared: &[String]) -> Vec<String> {
        let mut fields: Vec<String> = shared.iter().filter(|meaning| *meaning != "main_type" && *meaning != "sub_type" && layout::get(here, meaning) != layout::get(there, meaning)).cloned().collect();
        let category = |categories: &[Category], item: &ShopItem| category_name(categories, item.main_type, item.sub_type);
        if category(&self.categories, here) != category(&other.categories, there) {
            fields.push("category".into());
        }
        for field in &here.other {
            if there.other.iter().any(|source| source.path == field.path && source.ty == field.ty && source.value != field.value) {
                fields.push(field.path.clone());
            }
        }
        fields
    }

    pub fn compare(&self, other: &ComparedShop) -> ShopComparison {
        let here: Vec<ShopItem> = self.entries.iter().map(|entry| entry.item.clone()).collect();
        let shared = self.shared_meanings(other);
        let pairs = self.pairs(&here, other, &shared);
        let mut paired = vec![false; here.len()];
        let mut rows = Vec::new();
        let mut same = 0;
        for (there, item) in other.items.iter().enumerate() {
            let row = |status, here: Option<usize>, fields, blocked| CompareRow {
                status,
                here,
                there: Some(there),
                id: item.id,
                num: item.num,
                name: item.name.clone(),
                price: item.price,
                category: category_name(&other.categories, item.main_type, item.sub_type),
                fields,
                blocked,
            };
            let blocked = match place(&other.categories, &self.categories, item.main_type, item.sub_type) {
                Place::Blocked(reason) => Some(reason),
                _ => None,
            };
            match pairs[there] {
                Some(index) => {
                    paired[index] = true;
                    let fields = self.differing(&here[index], item, other, &shared);
                    if fields.is_empty() {
                        same += 1;
                    } else {
                        rows.push(row("different", Some(index), fields, blocked));
                    }
                }
                None => rows.push(row("missing", None, Vec::new(), blocked)),
            }
        }
        for (index, item) in here.iter().enumerate().filter(|(index, _)| !paired[*index]) {
            rows.push(CompareRow {
                status: "only_here",
                here: Some(index),
                there: None,
                id: item.id,
                num: item.num,
                name: item.name.clone(),
                price: item.price,
                category: category_name(&self.categories, item.main_type, item.sub_type),
                fields: Vec::new(),
                blocked: None,
            });
        }
        let mine = self.layout.meanings();
        ShopComparison {
            path: other.path.display().to_string(),
            json: other.json,
            kind: other.kind.clone(),
            layout: other.layout.clone(),
            items: other.items.len(),
            same,
            only_there: other.meanings.iter().filter(|meaning| !mine.contains(meaning)).cloned().collect(),
            only_here: mine.iter().filter(|meaning| !other.meanings.contains(meaning)).cloned().collect(),
            rows,
        }
    }

    /// Copies the picked items of the other shop (by their position there): paired items are replaced
    /// field by field, missing ones are added after the last item of their subcategory. Subcategories this
    /// shop lacks are added. One undo step.
    pub fn copy_compared(&mut self, other: &ComparedShop, picks: &[usize]) -> Result<(View, CopyReport), String> {
        let here: Vec<ShopItem> = self.entries.iter().map(|entry| entry.item.clone()).collect();
        let shared = self.shared_meanings(other);
        let pairs = self.pairs(&here, other, &shared);
        let mut picks = picks.to_vec();
        picks.sort_unstable();
        picks.dedup();
        let mut categories = self.categories.clone();
        let mut report = CopyReport { replaced: 0, added: 0, subcategories: Vec::new() };
        let mut replacements = Vec::new();
        let mut additions = Vec::new();
        let blank = layout::decode(&self.layout, &vec![0; self.layout.size()]);
        for &there in &picks {
            let item = other.items.get(there).ok_or_else(|| format!("The other shop has no item {}", there + 1))?;
            let (main, sub) = match place(&other.categories, &categories, item.main_type, item.sub_type) {
                Place::At(main, sub) => (main, sub),
                Place::NewSub(main, name) => {
                    categories[main].subs.push(name.clone());
                    report.subcategories.push(format!("{} › {name}", categories[main].name));
                    (main, categories[main].subs.len() as i32 - 1)
                }
                Place::Blocked(reason) => return Err(format!("Item {} ({}): {reason}", there + 1, item.name)),
            };
            match pairs[there] {
                Some(index) => replacements.push((index, merged(&here[index], item, &shared, main, sub))),
                None => additions.push(merged(&blank, item, &shared, main, sub)),
            }
        }
        if self.entries.len() + additions.len() > MAX_ITEMS {
            return Err(format!("At most {MAX_ITEMS} items"));
        }
        let mut changes = Vec::new();
        if categories != self.categories {
            changes.push(Change::Categories { before: self.categories.clone(), after: categories });
        }
        for (index, item) in replacements {
            let before = self.entries[index].clone();
            if before.item != item {
                changes.push(Change::Item { index, before: Some(before.clone()), after: Some(Entry { item, ..before }) });
                report.replaced += 1;
            }
        }
        // Places of the items as the additions go in.
        let mut places: Vec<(i32, i32)> = here.iter().map(|item| (item.main_type, item.sub_type)).collect();
        for item in additions {
            let key = (item.main_type, item.sub_type);
            let at = places.iter().rposition(|place| *place == key).or_else(|| places.iter().rposition(|place| place.0 == key.0)).map_or(places.len(), |index| index + 1);
            places.insert(at, key);
            changes.push(Change::Item { index: at, before: None, after: Some(Entry { item, raw: None }) });
            report.added += 1;
        }
        if changes.is_empty() {
            return Ok((self.view(), report));
        }
        let source = other.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let count = report.replaced + report.added;
        let view = self.record_all(format!("Copy {count} item{} from {source}", if count == 1 { "" } else { "s" }), changes)?;
        Ok((view, report))
    }

    /// A JSON export of the shop, or of the picked items (by position) with all categories.
    pub fn export_json(&self, picks: Option<&[usize]>) -> Result<(String, ExportCounts), String> {
        let items: Vec<ShopItem> = match picks {
            None => self.entries.iter().map(|entry| entry.item.clone()).collect(),
            Some(picks) => picks.iter().map(|&index| self.item(index)).collect::<Result<_, _>>()?,
        };
        let export = ShopJson {
            format: FORMAT.into(),
            version: 1,
            kind: shop_kind(&self.file.path).into(),
            timestamp: self.file.timestamp,
            layout: self.layout.id.clone(),
            meanings: self.layout.meanings(),
            categories: self.categories.clone(),
            items,
        };
        let counts = ExportCounts { items: export.items.len(), categories: export.categories.len() };
        let text = serde_json::to_string_pretty(&export).map_err(|error| error.to_string())?;
        Ok((text, counts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_and_copy_through_a_json_export() {
        let source = Path::new("E:/Game Dev/JD/1559/gamed/config/gshop.data");
        if !source.is_file() {
            return;
        }
        let layouts = layout::builtin();
        let folder = std::env::temp_dir().join(format!("jdide-gshop-compare-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("gshop.data");
        std::fs::copy(source, &path).unwrap();
        let mut document = Document::open(&path, &layouts, None).unwrap();
        let (json, counts) = document.export_json(None).unwrap();
        assert_eq!(counts.items, document.entries.len());
        let export = folder.join("shop.json");
        std::fs::write(&export, json).unwrap();
        let other = ComparedShop::open(&export, &layouts).unwrap();
        let fresh = document.compare(&other);
        assert_eq!((fresh.rows.len(), fresh.same), (0, other.items.len()), "a shop equals its own export");

        // Here: one item deleted, one price changed (in another subcategory), one subcategory renamed.
        let deleted = document.item(0).unwrap();
        let other_place = (1..document.entries.len()).find(|&index| { let item = &document.entries[index].item; (item.main_type, item.sub_type) != (deleted.main_type, deleted.sub_type) }).unwrap();
        let mut changed = document.item(other_place).unwrap();
        changed.price += 1;
        document.set_item(other_place, changed, "Edit price").unwrap();
        document.delete_item(0).unwrap();
        let main = deleted.main_type as usize;
        let sub = deleted.sub_type as usize;
        document.edit_categories(super::super::CategoryOp::RenameSub { main, sub, name: "Renamed".into() }).unwrap();
        let used = document.entries.iter().filter(|entry| entry.item.main_type == main as i32 && entry.item.sub_type == sub as i32).count();

        let comparison = document.compare(&other);
        let status = |name: &str| comparison.rows.iter().filter(|row| row.status == name).count();
        assert_eq!(status("missing"), 1);
        assert_eq!(status("only_here"), 0);
        // The price change, and the items in the renamed subcategory (their category name differs).
        assert_eq!(status("different"), 1 + used);
        let price = comparison.rows.iter().find(|row| row.fields.contains(&"price".to_string())).unwrap();
        assert_eq!(price.fields, vec!["price".to_string()]);

        let before = document.entries.len();
        let picks: Vec<usize> = comparison.rows.iter().filter_map(|row| row.there).collect();
        let (_, report) = document.copy_compared(&other, &picks).unwrap();
        assert_eq!((report.added, report.replaced), (1, 1 + used));
        assert_eq!(report.subcategories.len(), 1, "the old subcategory name comes back as a new one");
        assert_eq!(document.entries.len(), before + 1);
        let again = document.compare(&other);
        assert!(again.rows.is_empty(), "{:?}", again.rows.iter().take(3).collect::<Vec<_>>());
        // One undo step.
        document.undo().unwrap();
        assert_eq!(document.entries.len(), before);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn shops_of_other_layouts_compare_on_shared_fields() {
        let forsaken = Path::new("E:/Games/ForsakenJD/element/data/gshop.data");
        let hdn = Path::new("E:/Games/Elite Jade Dynasty - HDN/element/data/gshop.data");
        if !forsaken.is_file() || !hdn.is_file() {
            return;
        }
        let layouts = layout::builtin();
        let document = Document::open(forsaken, &layouts, None).unwrap();
        let other = ComparedShop::open(hdn, &layouts).unwrap();
        let comparison = document.compare(&other);
        assert_eq!(comparison.items, other.items.len());
        let paired = comparison.same + comparison.rows.iter().filter(|row| row.status == "different").count();
        assert!(paired > 0);
        assert!(comparison.rows.iter().all(|row| row.fields.iter().all(|field| field != "main_type" && field != "sub_type")));
    }
}
