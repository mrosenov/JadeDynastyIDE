//! `vipaward.data` (the client's `data\VIPAward.data`, the server's gs.conf `VipAwardData`): the daily and
//! special awards of the VIP award window.
//!
//! Layout (`LoadVIPAwardData` in ZCommon/globaldataman.cpp, the server's `load_vipaward_data`): u32 timestamp,
//! i32 count (0–65535), `count` × `VIP_AWARD_ITEM` (`#pragma pack(1)`, 156 bytes): u32 tid (award ID, unique,
//! increasing), WORD szName[64], u32 item_id, u32 count, u32 award_type (0 normal, 1 VIP), u32 award_level
//! (normal: a level band 1–8; VIP: the VIP level from 1), u32 award_obtain_type (0 daily, 1 special), i32
//! expire_time (seconds the item lasts; 0 = forever). No category block follows.
//!
//! 2018 builds (HDN, Reborn, zx_18; no source) append 16 bytes and add a third kind, the **VIP shop**
//! (award_type 2, obtain_type 2, levels 1–8): the client's vip_shop_item window shows a price (价格: the float
//! after the source record, 100–99,900), a purchase limit (限次: the next int, 1–3 in Reborn, 0 in HDN), the
//! level and the duration, with a Buy button. The last two ints are 0. VIP levels there go up to 8 (the
//! client names 10: ingame.stf 13020–13029).
//!
//! The server (zgame/gs playervipaward.cpp `CheckParam`, `CheckLevelAwardCnt`) refuses to start on an award
//! with ID, item or count ≤ 0, a type or obtain type other than 0/1, a normal level outside 1–8, a VIP level
//! outside 1–6 (2013 source), a repeated ID, or more than 16 awards of one kind and level. A claim sends the
//! award and item IDs, which must match the server's file. The client shows the item's own name (szName is
//! not displayed), the count, the duration and a daily/once mark, and lists a level's awards in file order.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

const HEADER_SIZE: usize = 8;
/// The record of the source; newer builds append bytes.
pub const BASE_SIZE: usize = 156;
const NAME_UNITS: usize = 64;
/// Awards of one kind and level the server accepts (`MAX_COUNT_PER_LEVEL`).
pub const MAX_PER_LEVEL: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Award {
    /// Award ID (`tid`): unique; the client sends it to claim the award.
    pub id: u32,
    pub name: String,
    pub item_id: u32,
    pub count: u32,
    /// 0 normal, 1 VIP, 2 VIP shop (newer builds).
    pub award_type: u32,
    /// Normal: level band 1–8 (4 bands, then the same 4 after rebirth); VIP: VIP level from 1.
    pub level: u32,
    /// 0 daily, 1 special (once), 2 bought in the VIP shop (newer builds).
    pub obtain_type: u32,
    /// Seconds the item lasts once given (0 = forever).
    pub expire_time: i32,
    /// Newer builds: the VIP shop price (a float after the source record), then the purchase limit and two
    /// unknown ints.
    #[serde(default)]
    pub price: Option<f32>,
    #[serde(default)]
    pub extra: Vec<i32>,
    /// The record as stored (names can keep bytes after their terminator); new awards copy their source's.
    #[serde(default)]
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub size: usize,
    pub timestamp: u32,
    pub record_size: usize,
    /// MD5 of the file as read (changed-on-disk guard).
    pub token: String,
    pub awards: Vec<Award>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequest {
    pub opened_path: String,
    pub target_path: String,
    pub token: String,
    pub record_size: usize,
    pub awards: Vec<Award>,
    pub backup: bool,
    pub replace_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub awards: usize,
    pub timestamp: u32,
    pub token: String,
    pub backup: Option<String>,
}

fn token(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn name_of(record: &[u8]) -> String {
    let units: Vec<u16> = record[4..4 + NAME_UNITS * 2].chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|&unit| unit != 0).collect();
    String::from_utf16_lossy(&units)
}

fn decode(record: &[u8]) -> Award {
    let size = record.len();
    Award {
        id: u32_at(record, 0),
        name: name_of(record),
        item_id: u32_at(record, 132),
        count: u32_at(record, 136),
        award_type: u32_at(record, 140),
        level: u32_at(record, 144),
        obtain_type: u32_at(record, 148),
        expire_time: u32_at(record, 152) as i32,
        price: (size >= BASE_SIZE + 4).then(|| f32::from_le_bytes(record[156..160].try_into().unwrap())),
        extra: if size > BASE_SIZE + 4 { record[160..].chunks_exact(4).map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap())).collect() } else { Vec::new() },
        raw: record.to_vec(),
    }
}

/// The fields as the editor shows them (the stored bytes aside).
fn same_fields(a: &Award, b: &Award) -> bool {
    let strip = |award: &Award| Award { raw: Vec::new(), price: award.price.map(|price| if price.is_nan() { 0.0 } else { price }), ..award.clone() };
    strip(a) == strip(b)
}

pub fn parse(path: String, data: Vec<u8>) -> Result<FileView, String> {
    if data.len() < HEADER_SIZE {
        return Err("The file is too short for a vipaward.data header".into());
    }
    let timestamp = u32_at(&data, 0);
    let count = u32_at(&data, 4) as i32;
    if !(0..=65535).contains(&count) {
        return Err(format!("Invalid award count {count}; the client and server read 0–65535"));
    }
    let count = count as usize;
    let body = data.len() - HEADER_SIZE;
    let record_size = if count == 0 {
        if body != 0 {
            return Err(format!("No awards, but {body} bytes follow the header"));
        }
        BASE_SIZE
    } else {
        if body % count != 0 {
            return Err(format!("{body} bytes do not divide into {count} awards"));
        }
        body / count
    };
    if record_size < BASE_SIZE || (record_size - BASE_SIZE) % 4 != 0 {
        return Err(format!("Awards of {record_size} bytes are not a known vipaward.data layout (the source has {BASE_SIZE}; newer builds 172)"));
    }
    let awards = data[HEADER_SIZE..].chunks_exact(record_size).map(decode).collect();
    Ok(FileView { path, size: data.len(), timestamp, record_size, token: token(&data), awards })
}

fn encode_award(award: &Award, size: usize) -> Result<Vec<u8>, String> {
    let mut record = vec![0u8; size];
    if award.raw.len() == size {
        record.copy_from_slice(&award.raw);
    }
    let what = format!("Award {}", award.id);
    record[0..4].copy_from_slice(&award.id.to_le_bytes());
    // The name slot keeps its stored bytes while the text is the same.
    if name_of(&record) != award.name || award.raw.len() != size {
        let units: Vec<u16> = award.name.encode_utf16().collect();
        if units.len() > NAME_UNITS {
            return Err(format!("{what}: the name has {} characters; at most {NAME_UNITS}", units.len()));
        }
        record[4..4 + NAME_UNITS * 2].fill(0);
        for (index, unit) in units.iter().enumerate() {
            record[4 + index * 2..6 + index * 2].copy_from_slice(&unit.to_le_bytes());
        }
    }
    for (at, value) in [(132, award.item_id), (136, award.count), (140, award.award_type), (144, award.level), (148, award.obtain_type), (152, award.expire_time as u32)] {
        record[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    if size >= BASE_SIZE + 4 {
        let stored = f32::from_le_bytes(record[156..160].try_into().unwrap());
        let value = award.price.unwrap_or(0.0);
        // Same value (or both NaN): keep the stored bits.
        if stored != value && !(stored.is_nan() && value.is_nan()) {
            record[156..160].copy_from_slice(&value.to_le_bytes());
        }
        let slots = (size - BASE_SIZE - 4) / 4;
        if award.extra.len() > slots {
            return Err(format!("{what} has {} unknown values; this file's awards hold {slots}", award.extra.len()));
        }
        for (index, value) in award.extra.iter().enumerate() {
            record[160 + index * 4..164 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(record)
}

/// The file for `awards` in their order (the order the client lists a level's awards in).
pub fn encode(awards: &[Award], timestamp: u32, record_size: usize) -> Result<Vec<u8>, String> {
    if awards.len() > 65535 {
        return Err("vipaward.data holds at most 65,535 awards".into());
    }
    let mut out = Vec::with_capacity(HEADER_SIZE + awards.len() * record_size);
    out.extend_from_slice(&timestamp.to_le_bytes());
    out.extend_from_slice(&(awards.len() as u32).to_le_bytes());
    for award in awards {
        out.extend_from_slice(&encode_award(award, record_size)?);
    }
    Ok(out)
}

pub fn open(path: String) -> Result<FileView, String> {
    let data = std::fs::read(&path).map_err(|error| format!("Could not read {path}: {error}"))?;
    parse(path, data)
}

pub fn save(request: SaveRequest) -> Result<SaveReport, String> {
    let target = PathBuf::from(&request.target_path);
    let opened = Path::new(&request.opened_path);
    let same = crate::path_data::same_path(&target, opened);
    let previous = std::fs::read(&target).ok();
    if same && !request.replace_changed {
        if let Some(current) = &previous {
            if token(current) != request.token {
                return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", target.display()));
            }
        }
    }
    // The timestamp is the export time (nothing compares it); keep it moving forward.
    let old = previous.as_deref().filter(|data| data.len() >= 4).map(|data| u32_at(data, 0)).unwrap_or(0);
    let timestamp = (Local::now().timestamp().max(0) as u32).max(old.saturating_add(1));
    let data = encode(&request.awards, timestamp, request.record_size)?;
    let back = parse(String::new(), data.clone())?;
    if back.awards.len() != request.awards.len() || back.awards.iter().zip(&request.awards).any(|(a, b)| !same_fields(a, b)) {
        return Err("The saved file would not read back as these awards; nothing was written".into());
    }
    let backup = if request.backup && target.is_file() { Some(crate::backup::archive(&target, std::slice::from_ref(&target))?) } else { None };
    crate::path_data::write_replacing(&target, &data)?;
    Ok(SaveReport { path: target.display().to_string(), size: data.len(), awards: request.awards.len(), timestamp, token: token(&data), backup: backup.map(|path| path.display().to_string()) })
}

// ── Problems ──

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `error`: the server refuses to start (or the claim fails); `warning`: probably wrong.
    pub severity: &'static str,
    /// Position of the award (none: the whole file).
    pub index: Option<usize>,
    pub message: String,
}

/// What the open elements.data says about an item: none when no elements.data is open.
pub struct ItemInfo {
    pub exists: bool,
    /// `pile_num_max` when the item has one.
    pub stack: Option<u32>,
}

/// Problems by the server's rules (`award_data::AddAward`, `CheckLevelAwardCnt`; a failure makes
/// `item_manager::InitFromDataMan` fail and gs exit with -8). `items`: what elements.data knows about each
/// item (none: not open).
pub fn problems(awards: &[Award], record_size: usize, items: Option<&HashMap<u32, ItemInfo>>) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut add = |severity, index: Option<usize>, message: String| out.push(Problem { severity, index, message });
    let newer = record_size > BASE_SIZE;
    let mut first_of: HashMap<u32, usize> = HashMap::new();
    let mut per_level: HashMap<(u32, u32, u32), Vec<usize>> = HashMap::new();
    for (index, award) in awards.iter().enumerate() {
        let at = Some(index);
        if award.id == 0 || award.id > i32::MAX as u32 {
            add("error", at, format!("Award ID {} must be above 0: the server refuses to start", award.id));
        } else if let Some(&first) = first_of.get(&award.id) {
            add("error", at, format!("Award ID {} is also award {}: the server refuses to start", award.id, first + 1));
        } else {
            first_of.insert(award.id, index);
        }
        if award.item_id == 0 || award.item_id > i32::MAX as u32 {
            add("error", at, "No item (ID 0): the server refuses to start".into());
        } else if let Some(info) = items.and_then(|items| items.get(&award.item_id)) {
            if !info.exists {
                add("error", at, format!("Item {} is not in the open elements.data: claiming it fails", award.item_id));
            } else if info.stack == Some(1) && award.count != 1 {
                // `AddAward` checks this while loading: item_manager init fails and gs exits with -8.
                add("error", at, format!("Item {} stacks to 1, so the count must be 1 (not {}): the server refuses to start (error -8)", award.item_id, award.count));
            }
        }
        if award.count == 0 || award.count > i32::MAX as u32 {
            add("error", at, "Count 0: the server refuses to start".into());
        }
        // Kinds: the source knows normal and VIP awards (daily or special); newer builds add the VIP shop.
        let kinds = if newer { 2 } else { 1 };
        if award.award_type > kinds {
            add("error", at, format!("Award type {} is not {}: the server refuses to start", award.award_type, if newer { "normal (0), VIP (1) or VIP shop (2)" } else { "normal (0) or VIP (1); the VIP shop (2) needs a newer build" }));
        }
        if award.obtain_type > kinds {
            add("error", at, format!("Obtain type {} is not {}: the server refuses to start", award.obtain_type, if newer { "daily (0), special (1) or shop (2)" } else { "daily (0) or special (1)" }));
        }
        if newer && (award.award_type == 2) != (award.obtain_type == 2) && award.award_type <= 2 && award.obtain_type <= 2 {
            add("warning", at, "VIP shop items have both the award type and the obtain type 2 in official files".into());
        }
        match award.award_type {
            0 if !(1..=8).contains(&award.level) => add("error", at, format!("Normal awards have levels 1–8, not {}: the server refuses to start", award.level)),
            1 | 2 if award.level == 0 => add("error", at, "VIP levels start at 1: the server refuses to start".into()),
            1 if award.level > 6 && !newer => add("error", at, format!("VIP level {}: the server source accepts 1–6 (newer builds with 172-byte awards use 7 and 8)", award.level)),
            1 | 2 if award.level > 8 => add("warning", at, format!("VIP level {}: newer builds use at most 8", award.level)),
            _ => {}
        }
        if award.award_type == 2 && award.price.is_some_and(|price| !(price > 0.0)) {
            add("warning", at, "A VIP shop item without a price".into());
        }
        if award.expire_time < 0 {
            add("warning", at, format!("Negative duration {}", award.expire_time));
        }
        // The limit of 16 per kind and level is the source's (normal and VIP awards).
        if award.award_type <= 1 && award.obtain_type <= 1 {
            per_level.entry((award.award_type, award.obtain_type, award.level)).or_default().push(index);
        }
    }
    let mut groups: Vec<_> = per_level.into_iter().filter(|(_, indexes)| indexes.len() > MAX_PER_LEVEL).collect();
    groups.sort();
    for ((award_type, obtain_type, level), indexes) in groups {
        add(
            "error",
            Some(indexes[MAX_PER_LEVEL]),
            format!("{} {} awards of level {level}: {} of at most {MAX_PER_LEVEL}; the server refuses to start", if award_type == 1 { "VIP" } else { "Normal" }, if obtain_type == 1 { "special" } else { "daily" }, indexes.len()),
        );
    }
    out
}

/// Item IDs the problems check needs looked up.
pub fn item_ids(awards: &[Award]) -> Vec<u32> {
    let set: HashSet<u32> = awards.iter().map(|award| award.item_id).filter(|&id| id != 0).collect();
    let mut ids: Vec<u32> = set.into_iter().collect();
    ids.sort_unstable();
    ids
}

/// The client's names of the level buttons (interfaces.pck `ingame.stf` 13011–13018 for normal level bands,
/// 13020–13029 for VIP levels), falling back to the English client's.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelNames {
    pub normal: Vec<String>,
    pub vip: Vec<String>,
    /// Read from the configured client.
    pub from_client: bool,
}

pub const NORMAL_NAMES: [&str; 8] = ["< LV90", "LV90-LV119", "LV120-LV134", "LV135-LV150", "<A.LV90", "90-119", "120-134", "135-160"];
pub const VIP_NAMES: [&str; 10] = ["Bronze", "Silver", "Gold", "Platinum", "Diamond", "VIP", "Silver VIP", "Gold VIP", "Platinum VIP", "Diamond VIP"];

pub fn level_names(strings: Option<&HashMap<u32, String>>) -> LevelNames {
    let pick = |base: u32, defaults: &[&str]| -> Vec<String> { defaults.iter().enumerate().map(|(index, default)| strings.and_then(|strings| strings.get(&(base + index as u32)).cloned()).unwrap_or_else(|| default.to_string())).collect() };
    LevelNames { normal: pick(13011, &NORMAL_NAMES), vip: pick(13020, &VIP_NAMES), from_client: strings.is_some() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<PathBuf> {
        [
            "E:/Games/ForsakenJD/element/data/vipaward.data",
            "E:/Games/XtremeJade/element/data/vipaward.data",
            "E:/Games/Elite Jade Dynasty - HDN/element/data/vipaward.data",
            "E:/Games/Jade Dynasty Reborn/element/data/vipaward.data",
            "E:/Game Dev/JD/1559/gamed/config/VIPAward.data",
            "E:/Game Dev/JD/zxserver/zgame/gs/config/vipaward.data",
            "E:/Game Dev/JD/zx_18_compiled/gamed/config/VIPAward.data",
        ]
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect()
    }

    #[test]
    fn samples_read_and_write_back_byte_for_byte() {
        for path in samples() {
            let data = std::fs::read(&path).unwrap();
            let file = parse(path.display().to_string(), data.clone()).unwrap();
            assert!(matches!(file.record_size, 156 | 172), "{}", path.display());
            assert_eq!(encode(&file.awards, file.timestamp, file.record_size).unwrap(), data, "{}", path.display());
            // Rebuilt from the fields alone (no stored bytes), every field reads back.
            let bare: Vec<Award> = file.awards.iter().map(|award| Award { raw: Vec::new(), ..award.clone() }).collect();
            let rebuilt = parse(String::new(), encode(&bare, file.timestamp, file.record_size).unwrap()).unwrap();
            assert!(rebuilt.awards.iter().zip(&file.awards).all(|(a, b)| same_fields(a, b)));
            // Official files break none of the server's rules.
            let errors: Vec<Problem> = problems(&file.awards, file.record_size, None).into_iter().filter(|problem| problem.severity == "error").collect();
            assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        }
    }

    #[test]
    fn edits_save_with_a_backup_and_refuse_outside_changes() {
        let Some(source) = samples().into_iter().next() else { return };
        let folder = std::env::temp_dir().join(format!("jdide-vipaward-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("vipaward.data");
        std::fs::copy(&source, &path).unwrap();
        let opened = open(path.display().to_string()).unwrap();
        let mut awards = opened.awards.clone();
        // A clone with the next ID, an edited name and level.
        let mut clone = awards[0].clone();
        clone.id = awards.iter().map(|award| award.id).max().unwrap() + 1;
        clone.name = "Edited award".into();
        awards.push(clone.clone());
        awards[1].count = 7;
        let request = SaveRequest { opened_path: opened.path.clone(), target_path: opened.path.clone(), token: opened.token.clone(), record_size: opened.record_size, awards: awards.clone(), backup: true, replace_changed: false };
        let report = save(request.clone()).unwrap();
        assert!(report.backup.is_some() && report.timestamp > opened.timestamp);
        let reopened = open(path.display().to_string()).unwrap();
        assert_eq!(reopened.awards.len(), opened.awards.len() + 1);
        assert_eq!(reopened.awards.last().unwrap().name, "Edited award");
        assert_eq!(reopened.awards[1].count, 7);
        // Untouched awards keep their stored bytes (names with bytes after the terminator).
        assert_eq!(reopened.awards[2].raw, opened.awards[2].raw);
        // The token of the first open no longer matches.
        assert!(save(request).unwrap_err().starts_with("CHANGED_ON_DISK"));
        // Server rules.
        let mut bad = reopened.awards.clone();
        bad[0].level = 9;
        bad[1].id = bad[2].id;
        // An item that stacks to 1 with a count above 1 stops the server (a user saved Iron Sword × 99: error -8).
        let items: HashMap<u32, ItemInfo> = [(bad[3].item_id, ItemInfo { exists: true, stack: Some(1) })].into_iter().collect();
        bad[3].count = 99;
        assert!(problems(&bad, reopened.record_size, Some(&items)).iter().any(|problem| problem.index == Some(3) && problem.message.contains("refuses to start (error -8)")));
        let found = problems(&bad, reopened.record_size, None);
        assert!(found.iter().any(|problem| problem.index == Some(0) && problem.message.contains("levels 1–8")) || found.iter().any(|problem| problem.index == Some(0) && problem.message.contains("VIP level")));
        assert!(found.iter().any(|problem| problem.index == Some(2) && problem.message.contains("also award")));
        let _ = std::fs::remove_dir_all(&folder);
    }
}

