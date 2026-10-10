//! `task_npc.data`: where the quest tracker finds NPCs and monsters.
//!
//! Layout (`ATaskTemplMan::UnmarshalNPCInfo`): a 12-byte header (u32 pack_size = file size, i32
//! time_mark, u16 version 2, u16 count), then `count` NPC_INFO records of 16 bytes: u32 id (an NPC or
//! monster template), i32 map_id (instance; 0 = unknown), i16 x, y, z, and 2 padding bytes (the
//! struct is declared before `#pragma pack(1)`). The client loads `data\task_npc.data` for the
//! tracker and minimap links; the server loads its own copy (gs.conf `QuestNPCInfo`) to teleport
//! players to an NPC. Both keep the last record of an ID.

use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};

pub const VERSION: u16 = 2;
const HEADER_SIZE: usize = 12;
const RECORD_SIZE: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: u32,
    pub map: i32,
    pub x: i16,
    pub y: i16,
    pub z: i16,
    /// The record's padding bytes, kept as they were (always 0 in official files).
    #[serde(default)]
    pub pad: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub size: usize,
    pub time_mark: i32,
    /// MD5 of the file as read (changed-on-disk guard).
    pub token: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequest {
    pub opened_path: String,
    pub target_path: String,
    pub token: String,
    pub rows: Vec<Row>,
    pub backup: bool,
    pub replace_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub rows: usize,
    pub time_mark: i32,
    pub token: String,
    pub backup: Option<String>,
}

fn token(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn parse(path: String, data: Vec<u8>) -> Result<FileView, String> {
    if data.len() < HEADER_SIZE {
        return Err("The file is too short for a task_npc.data header".into());
    }
    let pack_size = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let time_mark = i32::from_le_bytes(data[4..8].try_into().unwrap());
    let version = u16::from_le_bytes(data[8..10].try_into().unwrap());
    let count = u16::from_le_bytes(data[10..12].try_into().unwrap()) as usize;
    if version != VERSION {
        return Err(format!("task_npc.data version {version} is not supported (the client reads version {VERSION})"));
    }
    if pack_size != data.len() {
        return Err(format!("The header says {pack_size} bytes but the file has {} (the client refuses it)", data.len()));
    }
    if HEADER_SIZE + count * RECORD_SIZE != data.len() {
        return Err(format!("{count} records need {} bytes, but the file has {}", HEADER_SIZE + count * RECORD_SIZE, data.len()));
    }
    let rows = data[HEADER_SIZE..].chunks_exact(RECORD_SIZE).map(|record| Row {
        id: u32::from_le_bytes(record[0..4].try_into().unwrap()),
        map: i32::from_le_bytes(record[4..8].try_into().unwrap()),
        x: i16::from_le_bytes(record[8..10].try_into().unwrap()),
        y: i16::from_le_bytes(record[10..12].try_into().unwrap()),
        z: i16::from_le_bytes(record[12..14].try_into().unwrap()),
        pad: u16::from_le_bytes(record[14..16].try_into().unwrap()),
    }).collect();
    Ok(FileView { path, size: data.len(), time_mark, token: token(&data), rows })
}

/// The file for `rows` in their order. IDs must be non-zero and unique (the game would keep only
/// the last record of an ID).
pub fn encode(rows: &[Row], time_mark: i32) -> Result<Vec<u8>, String> {
    if rows.len() > u16::MAX as usize {
        return Err(format!("task_npc.data holds at most {} records", u16::MAX));
    }
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        if row.id == 0 {
            return Err("A record has ID 0".into());
        }
        if !seen.insert(row.id) {
            return Err(format!("ID {} is listed twice; the game would only keep the last one", row.id));
        }
    }
    let size = HEADER_SIZE + rows.len() * RECORD_SIZE;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&time_mark.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    for row in rows {
        out.extend_from_slice(&row.id.to_le_bytes());
        out.extend_from_slice(&row.map.to_le_bytes());
        out.extend_from_slice(&row.x.to_le_bytes());
        out.extend_from_slice(&row.y.to_le_bytes());
        out.extend_from_slice(&row.z.to_le_bytes());
        out.extend_from_slice(&row.pad.to_le_bytes());
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
    // A new time mark, moving forward from the replaced file's.
    let old_mark = previous.as_deref().filter(|data| data.len() >= 8).map(|data| i32::from_le_bytes(data[4..8].try_into().unwrap())).unwrap_or(0);
    let time_mark = (Local::now().timestamp() as i32).max(old_mark.saturating_add(1));
    let data = encode(&request.rows, time_mark)?;
    if parse(String::new(), data.clone())?.rows != request.rows {
        return Err("The saved table would not read back as these rows; nothing was written".into());
    }
    let backup = if request.backup && target.is_file() {
        Some(crate::backup::archive(&target, std::slice::from_ref(&target))?)
    } else {
        None
    };
    crate::path_data::write_replacing(&target, &data)?;
    Ok(SaveReport { path: target.display().to_string(), size: data.len(), rows: request.rows.len(), time_mark, token: token(&data), backup: backup.map(|path| path.display().to_string()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLES: [&str; 6] = [
        "E:/Games/XtremeJade/element/data/task_npc.data",
        "E:/Game Dev/JD/1559/gamed/config/task_npc.data",
        "E:/Games/ForsakenJD/element/data/task_npc.data",
        "E:/Games/Elite Jade Dynasty - HDN/element/data/task_npc.data",
        "E:/Games/Jade Dynasty Reborn/element/data/task_npc.data",
        "E:/Game Dev/JD/zxserver/zgame/gs/config/task_npc.data",
    ];

    #[test]
    fn real_tables_read_and_write_byte_for_byte() {
        for path in SAMPLES {
            let Ok(data) = std::fs::read(path) else { continue };
            let view = parse(path.into(), data.clone()).unwrap_or_else(|error| panic!("{path}: {error}"));
            assert!(view.rows.len() > 2000, "{path}");
            assert_eq!(encode(&view.rows, view.time_mark).unwrap(), data, "{path}");
        }
    }

    #[test]
    fn saving_checks_ids_and_moves_the_time_mark() {
        let rows = vec![Row { id: 7, map: 401, x: 231, y: 457, z: 468, pad: 0 }, Row { id: 8, map: 0, x: 0, y: 0, z: 0, pad: 0 }];
        assert!(encode(&[rows[0].clone(), rows[0].clone()], 0).is_err());
        assert!(encode(&[Row { id: 0, ..rows[0].clone() }], 0).is_err());
        let target = std::env::temp_dir().join(format!("jdide-task-npc-{}.data", std::process::id()));
        std::fs::write(&target, encode(&rows, 100).unwrap()).unwrap();
        let opened = open(target.display().to_string()).unwrap();
        let mut next = opened.rows.clone();
        next[1].map = 1;
        next.push(Row { id: 9, map: 2, x: -5, y: 10, z: 20, pad: 0 });
        let report = save(SaveRequest { opened_path: opened.path.clone(), target_path: opened.path.clone(), token: opened.token.clone(), rows: next.clone(), backup: false, replace_changed: false }).unwrap();
        assert!(report.time_mark > 100);
        assert_eq!(open(target.display().to_string()).unwrap().rows, next);
        // The old token no longer matches.
        assert!(save(SaveRequest { opened_path: opened.path.clone(), target_path: opened.path, token: opened.token, rows: next, backup: false, replace_changed: false }).unwrap_err().starts_with("CHANGED_ON_DISK"));
        let _ = std::fs::remove_file(&target);
    }
}
