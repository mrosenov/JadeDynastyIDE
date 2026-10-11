//! extra_drops.sev: the server's extra drop tables (gs.conf `[Template] DropData`; server
//! template/extra_drop_table.cpp, global_drop.cpp; official editor ZElementData MounsterFallListDlg, export
//! `CMounsterResetFallListData::Save`).
//!
//! ```text
//! u32 version (1; the zx_18 server also reads 2)  i32 count
//! count × { i32 n, n × u32 monster IDs; 256 × { u32 item ID, f32 chance }; char name[128] (GBK); i32 type
//!           (1 replace the monster's own drops, 2 add to them); 8 × f32 chances of dropping 0–7 items;
//!           [version 2: u32, read and ignored by the zx_18 source] }
//! ```
//!
//! `drop_template::LoadDropList` stops gs (-6) when a type is not 1 or 2, the count chances or the item chances
//! (up to the first item ID 0) do not add up to 1 within 1e-5 (summed as f32), or a monster is in two tables
//! (or twice in one). A missing file or another version stops it with -5 (the 2013 server reads version 1 only).
//! On a kill the server rolls the monster's drop rate, then the count, then each item by its chance; type 1 skips
//! the monster's own elements.data drops.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use encoding_rs::GBK;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

pub const SLOTS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DropItem {
    pub id: u32,
    pub chance: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropTable {
    pub name: String,
    /// The stored 128-byte name, kept while it still says `name`.
    #[serde(default)]
    pub name_raw: Vec<u8>,
    /// 1 replace, 2 add.
    #[serde(rename = "type")]
    pub kind: i32,
    pub monsters: Vec<u32>,
    /// The item slots up to the last one that is not empty (the file always has 256; the rest are zeros).
    pub items: Vec<DropItem>,
    /// Chances of dropping 0–7 items.
    pub counts: [f32; 8],
    /// Version 2 only: a value the zx_18 server source reads and ignores.
    #[serde(default)]
    pub extra: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DropFile {
    pub version: u32,
    pub tables: Vec<DropTable>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDrops {
    pub path: String,
    pub size: usize,
    pub token: String,
    pub file: DropFile,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, len: usize, what: &str) -> Result<&[u8], String> {
        let end = self.at.checked_add(len).filter(|end| *end <= self.data.len()).ok_or_else(|| format!("{what} runs past the end of the file (offset {})", self.at))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn u32(&mut self, what: &str) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn f32(&mut self, what: &str) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
}

fn gbk_name(raw: &[u8]) -> String {
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    GBK.decode_without_bom_handling(&raw[..end]).0.into_owned()
}

pub fn parse(data: &[u8]) -> Result<DropFile, String> {
    let mut reader = Reader { data, at: 0 };
    let version = reader.u32("the version")?;
    if version != 1 && version != 2 {
        return Err(format!("version {version}: servers read version 1 (and the zx_18 server 2)"));
    }
    let count = reader.u32("the table count")? as usize;
    if count > 100_000 {
        return Err(format!("{count} tables do not fit the file"));
    }
    let mut tables = Vec::with_capacity(count);
    for index in 0..count {
        let what = format!("table {}", index + 1);
        let monsters = reader.u32(&what)? as usize;
        if monsters > (data.len() - reader.at) / 4 {
            return Err(format!("{what}: {monsters} monsters do not fit the file"));
        }
        let monsters = (0..monsters).map(|_| reader.u32(&what)).collect::<Result<Vec<_>, String>>()?;
        let mut items = (0..SLOTS).map(|_| Ok(DropItem { id: reader.u32(&what)?, chance: reader.f32(&what)? })).collect::<Result<Vec<_>, String>>()?;
        while items.last().is_some_and(|item| item.id == 0 && item.chance.to_bits() == 0) {
            items.pop();
        }
        let name_raw = reader.take(128, &what)?.to_vec();
        let kind = reader.u32(&what)? as i32;
        let mut counts = [0f32; 8];
        for count in &mut counts {
            *count = reader.f32(&what)?;
        }
        let extra = if version == 2 { reader.u32(&what)? } else { 0 };
        tables.push(DropTable { name: gbk_name(&name_raw), name_raw, kind, monsters, items, counts, extra });
    }
    if reader.at != data.len() {
        return Err(format!("{} bytes follow the last table", data.len() - reader.at));
    }
    Ok(DropFile { version, tables })
}

fn name_slot(table: &DropTable) -> Result<Vec<u8>, String> {
    if table.name_raw.len() == 128 && gbk_name(&table.name_raw) == table.name {
        return Ok(table.name_raw.clone());
    }
    let (encoded, _, unmappable) = GBK.encode(&table.name);
    if unmappable {
        return Err(format!("“{}”: the name has characters GBK cannot store", table.name));
    }
    if encoded.len() > 127 {
        return Err(format!("“{}”: the name is {} GBK bytes; at most 127", table.name, encoded.len()));
    }
    let mut slot = vec![0u8; 128];
    slot[..encoded.len()].copy_from_slice(&encoded);
    Ok(slot)
}

pub fn encode(file: &DropFile) -> Result<Vec<u8>, String> {
    if file.version != 1 && file.version != 2 {
        return Err(format!("Version {} cannot be written (1 or 2)", file.version));
    }
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    put(&mut out, file.version);
    put(&mut out, file.tables.len() as u32);
    for (index, table) in file.tables.iter().enumerate() {
        if table.items.len() > SLOTS {
            return Err(format!("Table {} ({}): {} items; a table holds {SLOTS}", index + 1, table.name, table.items.len()));
        }
        put(&mut out, table.monsters.len() as u32);
        for &monster in &table.monsters {
            put(&mut out, monster);
        }
        for slot in 0..SLOTS {
            let item = table.items.get(slot).copied().unwrap_or(DropItem { id: 0, chance: 0.0 });
            put(&mut out, item.id);
            out.extend_from_slice(&item.chance.to_le_bytes());
        }
        out.extend_from_slice(&name_slot(table)?);
        put(&mut out, table.kind as u32);
        for count in table.counts {
            out.extend_from_slice(&count.to_le_bytes());
        }
        if file.version == 2 {
            put(&mut out, table.extra);
        }
    }
    Ok(out)
}

fn token(data: &[u8]) -> String {
    format!("{:x}", Md5::digest(data))
}

pub fn open(path: &Path) -> Result<OpenedDrops, String> {
    let data = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let file = parse(&data).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(OpenedDrops { path: path.display().to_string(), size: data.len(), token: token(&data), file })
}

// ── Problems ──

/// The items the server keeps: up to the first item ID 0 (`LoadDropList`).
pub fn used_items(table: &DropTable) -> &[DropItem] {
    let end = table.items.iter().position(|item| item.id == 0).unwrap_or(table.items.len());
    &table.items[..end]
}

/// A sum as the server adds it: f32, in order.
fn f32_sum(values: impl Iterator<Item = f32>) -> f32 {
    values.fold(0f32, |sum, value| sum + value)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `error`: gs does not start (-6); `warning`: the table misbehaves; `note`: probably unintended.
    pub severity: &'static str,
    pub table: Option<usize>,
    pub message: String,
}

/// What the open elements.data knows, for ID checks (none: not open).
#[derive(Default)]
pub struct Known {
    pub monsters: Option<HashSet<u32>>,
    pub items: Option<HashSet<u32>>,
}

pub fn problems(file: &DropFile, known: &Known) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut owners: HashMap<u32, usize> = HashMap::new();
    for (index, table) in file.tables.iter().enumerate() {
        let mut add = |severity, message: String| out.push(Problem { severity, table: Some(index), message });
        if table.kind != 1 && table.kind != 2 {
            add("error", format!("Type {} is not 1 (replace) or 2 (add): gs does not start (-6)", table.kind));
        }
        let counts = f32_sum(table.counts.iter().copied());
        if (1.0 - counts).abs() > 1e-5 {
            add("error", format!("The drop count chances add up to {}%, not 100%: gs does not start (-6)", round_percent(counts)));
        }
        let used = used_items(table);
        let items = f32_sum(used.iter().map(|item| item.chance));
        if (1.0 - items).abs() > 1e-5 {
            add("error", format!("The item chances add up to {}%, not 100%: gs does not start (-6)", round_percent(items)));
        }
        if used.is_empty() && table.counts[1..].iter().any(|&count| count > 0.0) {
            add("error", "The table can drop items but has none: the server picks from an empty list (it may crash)".into());
        }
        let ignored = table.items.iter().skip(used.len()).filter(|item| item.id != 0).count();
        if ignored > 0 {
            add("warning", format!("{ignored} item{} after an empty slot {} ignored by the server (it stops at the first empty slot)", if ignored == 1 { "" } else { "s" }, if ignored == 1 { "is" } else { "are" }));
        }
        if table.counts.iter().chain(used.iter().map(|item| &item.chance)).any(|&value| value < 0.0 || !value.is_finite()) {
            add("warning", "A chance is negative or not a number".into());
        }
        if table.monsters.is_empty() {
            add("note", "No monster uses this table".into());
        }
        let mut seen = HashSet::new();
        for &monster in &table.monsters {
            if !seen.insert(monster) {
                add("error", format!("Monster {monster} is listed twice in this table: gs does not start (-6)"));
            } else if let Some(&other) = owners.get(&monster) {
                add("error", format!("Monster {monster} is also in table {} ({}): gs does not start (-6)", other + 1, file.tables[other].name));
            }
            if known.monsters.as_ref().is_some_and(|known| !known.contains(&monster)) {
                add("warning", format!("Monster {monster} is not a monster of the open elements.data"));
            }
        }
        for &monster in &table.monsters {
            owners.entry(monster).or_insert(index);
        }
        if let Some(known) = &known.items {
            let missing: Vec<String> = used.iter().filter(|item| !known.contains(&item.id)).map(|item| item.id.to_string()).collect();
            if !missing.is_empty() {
                add("warning", format!("Item{} {} {} not in the open elements.data", if missing.len() == 1 { "" } else { "s" }, missing.iter().take(8).cloned().collect::<Vec<_>>().join(", "), if missing.len() == 1 { "is" } else { "are" }));
            }
        }
    }
    if file.version == 2 {
        out.push(Problem { severity: "note", table: None, message: "Version 2: the 2013 server reads version 1 only (gs stops with -5); the zx_18 server reads both".into() });
    }
    out
}

fn round_percent(value: f32) -> String {
    let percent = value as f64 * 100.0;
    format!("{}", (percent * 10_000.0).round() / 10_000.0)
}

// ── Index (for elements.data records) ──

/// Which tables name a monster or an item, from the saved file.
#[derive(Debug, Default)]
pub struct DropIndex {
    /// Monster ID → (table position, name, type, items).
    pub monsters: HashMap<u32, (usize, String, i32, usize)>,
    /// Item ID → table names.
    pub items: HashMap<u32, Vec<String>>,
}

pub fn index(file: &DropFile) -> DropIndex {
    let mut out = DropIndex::default();
    for (position, table) in file.tables.iter().enumerate() {
        let used = used_items(table);
        for &monster in &table.monsters {
            out.monsters.entry(monster).or_insert_with(|| (position, table.name.clone(), table.kind, used.len()));
        }
        let mut seen = HashSet::new();
        for item in used {
            if seen.insert(item.id) {
                out.items.entry(item.id).or_default().push(table.name.clone());
            }
        }
    }
    out
}

/// The hint for a record's ID: the table a monster uses, or the tables that drop an item.
pub fn hint(index: &DropIndex, id: u32, monster: bool) -> Option<String> {
    if monster {
        let (_, name, kind, items) = index.monsters.get(&id)?;
        return Some(format!("Extra drops: {name} ({}, {items} item{})", if *kind == 1 { "replaces its drops" } else { "adds to its drops" }, if *items == 1 { "" } else { "s" }));
    }
    let tables = index.items.get(&id)?;
    Some(format!("Dropped by {} extra drop table{}: {}{}", tables.len(), if tables.len() == 1 { "" } else { "s" }, tables.iter().take(3).cloned().collect::<Vec<_>>().join(", "), if tables.len() > 3 { ", …" } else { "" }))
}

// ── Saving ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub token: String,
    pub backup: Option<String>,
}

/// Writes the tables: they must read back the same; another program's changes are refused unless
/// `replace_changed`; a 7z backup of the replaced file.
pub fn save(path: &Path, token_now: Option<&str>, file: &DropFile, backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
    let data = encode(file)?;
    let mut expected = file.clone();
    for table in &mut expected.tables {
        table.name_raw = name_slot(table)?;
        while table.items.last().is_some_and(|item| item.id == 0 && item.chance.to_bits() == 0) {
            table.items.pop();
        }
        if file.version == 1 {
            table.extra = 0;
        }
    }
    if parse(&data)? != expected {
        return Err("The written file would not read back as these tables; nothing was saved".into());
    }
    if !replace_changed {
        if let (Some(expected), Ok(current)) = (token_now, std::fs::read(path)) {
            if token(&current) != expected {
                return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", path.display()));
            }
        }
    }
    let target: PathBuf = path.to_path_buf();
    let backup = if backup && target.is_file() { Some(crate::backup::archive(&target, std::slice::from_ref(&target))?.display().to_string()) } else { None };
    crate::path_data::write_replacing(&target, &data)?;
    Ok(SaveReport { path: target.display().to_string(), size: data.len(), token: token(&data), backup })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<&'static str> {
        vec!["E:/Game Dev/JD/zxserver/zgame/gs/config/extra_drops.sev", "E:/Game Dev/JD/1559/gamed/config/extra_drops.sev", "E:/Game Dev/JD/zx_18_compiled/gamed/config/extra_drops.sev"]
    }

    #[test]
    fn samples_read_back_byte_for_byte_and_pass_the_server_checks() {
        for path in samples() {
            let Ok(data) = std::fs::read(path) else { continue };
            let file = parse(&data).unwrap();
            assert_eq!(encode(&file).unwrap(), data, "{path}");
            let found = problems(&file, &Known::default());
            let errors: Vec<&Problem> = found.iter().filter(|problem| problem.severity == "error").collect();
            println!("DROPS {path}: v{} {} tables, {} problems, {} errors", file.version, file.tables.len(), found.len(), errors.len());
            assert!(errors.is_empty(), "{:?}", errors.first());
            // JSON round trip (the UI's copy) keeps every byte.
            let json = serde_json::to_string(&file).unwrap();
            let back: DropFile = serde_json::from_str(&json).unwrap();
            assert_eq!(encode(&back).unwrap(), data);
        }
    }

    #[test]
    fn index_names_tables_of_monsters_and_items() {
        let Some(path) = samples().into_iter().find(|path| Path::new(path).is_file()) else { return };
        let file = open(Path::new(path)).unwrap().file;
        let index = index(&file);
        let table = &file.tables[0];
        let monster = hint(&index, table.monsters[0], true).unwrap();
        assert!(monster.contains(&table.name), "{monster}");
        let item = hint(&index, used_items(table)[0].id, false).unwrap();
        assert!(item.starts_with("Dropped by"), "{item}");
        assert!(hint(&index, 0, true).is_none());
    }

    #[test]
    fn server_checks_and_save() {
        let Some(path) = samples().into_iter().find(|path| Path::new(path).is_file()) else { return };
        let mut file = open(Path::new(path)).unwrap().file;
        file.tables[0].kind = 3;
        file.tables[1].counts[0] += 0.01;
        file.tables[2].items[0].chance += 0.01;
        let first = file.tables[0].monsters[0];
        file.tables[3].monsters.push(first);
        let found = problems(&file, &Known::default());
        for table in 0..4 {
            assert!(found.iter().any(|problem| problem.table == Some(table) && problem.severity == "error"), "table {table}");
        }
        // Save into a temporary copy.
        let dir = std::env::temp_dir().join(format!("jdide-drops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("extra_drops.sev");
        std::fs::copy(path, &target).unwrap();
        let opened = open(&target).unwrap();
        let mut edited = opened.file.clone();
        edited.tables[0].name = "Edited table".into();
        edited.version = 1;
        let report = save(&target, Some(&opened.token), &edited, true, false).unwrap();
        assert!(report.backup.is_some());
        let back = open(&target).unwrap();
        assert_eq!(back.file.tables[0].name, "Edited table");
        assert_eq!(back.file.version, 1);
        assert!(save(&target, Some(&opened.token), &edited, false, false).unwrap_err().starts_with("CHANGED_ON_DISK:"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
