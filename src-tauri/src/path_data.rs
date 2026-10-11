//! Strict reader and writer for the client's `path.data` resource table.
//!
//! The client accepts only one small binary format:
//!
//! ```text
//! u32 0x504d4944, u32 count, count × { u32 id, u32 byte_len, GBK bytes }
//! ```
//!
//! Its loader rejects duplicate IDs and duplicate paths and reads each path
//! into a 256-byte buffer. The editor therefore caps encoded paths at 255
//! bytes and validates the complete table before replacing a file.
//!
//! `DynamicObjects.data` (the models and effects of npcgen.data's dynamic
//! objects) is read by the same client class (`DataPathMan`), so it shares
//! this reader and writer as [`Table::DynamicObjects`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::Local;
use encoding_rs::GBK;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

const MAGIC: u32 = 0x504d_4944;
const MAX_PATH_BYTES: usize = 255;
const JSON_FORMAT_VERSION: u32 = 1;

/// Which client table: both are `DataPathMan` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    Paths,
    DynamicObjects,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Table::Paths => "path.data",
            Table::DynamicObjects => "DynamicObjects.data",
        }
    }

    fn json_format(self) -> &'static str {
        match self {
            Table::Paths => "jdide.path-data",
            Table::DynamicObjects => "jdide.dynamic-objects",
        }
    }

    /// Official DynamicObjects.data keeps one entry with an empty path (ID 37 in every sample); the loader takes it
    /// (paths only have to be unique). The path.data editor keeps refusing empty paths.
    fn allows_empty(self) -> bool {
        self == Table::DynamicObjects
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: u32,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub size: usize,
    pub token: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveRequest {
    /// File that supplied `token`; used for the changed-on-disk guard.
    pub opened_path: String,
    pub target_path: String,
    pub token: String,
    pub rows: Vec<Row>,
    #[serde(default)]
    pub backup: bool,
    #[serde(default)]
    pub replace_changed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    pub path: String,
    pub size: usize,
    pub rows: usize,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// Saving this table changes the digest this sibling file must carry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sibling_elements: Option<String>,
    /// The configured client's cached paths/packages were refreshed.
    pub client_reloaded: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonReport {
    pub path: String,
    pub rows: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonImport {
    pub rows: Vec<Row>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonExport<'a> {
    format: &'static str,
    format_version: u32,
    source_path: &'a str,
    exported_at: String,
    records: &'a [Row],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonEnvelope {
    format: String,
    format_version: u32,
    #[serde(default)]
    source_path: Option<String>,
    #[serde(default)]
    exported_at: Option<String>,
    records: Vec<Row>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum JsonInput {
    Envelope(JsonEnvelope),
    Rows(Vec<Row>),
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4).map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn token(data: &[u8]) -> String {
    format!("{:x}", Md5::digest(data))
}

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn validate(rows: &[Row], table: Table) -> Result<Vec<(u32, Vec<u8>)>, String> {
    if rows.len() > u32::MAX as usize {
        return Err(format!("{} has too many rows", table.name()));
    }
    let mut ids = HashSet::with_capacity(rows.len());
    let mut paths = HashSet::with_capacity(rows.len());
    let mut encoded = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let number = index + 1;
        if row.id == 0 {
            return Err(format!("Row {number}: ID 0 is reserved for ‘not found’"));
        }
        if !ids.insert(row.id) {
            return Err(format!("Row {number}: ID {} is used more than once", row.id));
        }
        if row.path.is_empty() && !table.allows_empty() {
            return Err(format!("Row {number}: path is empty"));
        }
        if row.path.contains('\0') {
            return Err(format!("Row {number}: path contains a zero character"));
        }
        if !paths.insert(row.path.clone()) {
            return Err(format!("Row {number}: path “{}” is used more than once", row.path));
        }
        let (bytes, _, unmappable) = GBK.encode(&row.path);
        if unmappable {
            return Err(format!("Row {number}: path contains characters GBK cannot store"));
        }
        if bytes.len() > MAX_PATH_BYTES {
            return Err(format!("Row {number}: path takes {} GBK bytes; the client safely holds at most {MAX_PATH_BYTES}", bytes.len()));
        }
        encoded.push((row.id, bytes.into_owned()));
    }
    Ok(encoded)
}

fn encode(rows: &[Row], table: Table) -> Result<Vec<u8>, String> {
    let mut encoded = validate(rows, table)?;
    encoded.sort_unstable_by_key(|(id, _)| *id);
    let size = 8usize + encoded.iter().map(|(_, path)| 8 + path.len()).sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    for (id, path) in encoded {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(path.len() as u32).to_le_bytes());
        out.extend_from_slice(&path);
    }
    Ok(out)
}

fn parse(path: String, data: Vec<u8>, table: Table) -> Result<FileView, String> {
    let name = table.name();
    if data.len() < 8 || u32_at(&data, 0) != Some(MAGIC) {
        return Err(format!("{name}: wrong file signature (expected 0x504D4944)"));
    }
    let count = u32_at(&data, 4).unwrap() as usize;
    if count > (data.len() - 8) / 8 {
        return Err(format!("{name}: header declares {count} rows, but the file is too short"));
    }
    let mut rows = Vec::with_capacity(count);
    let mut at = 8usize;
    for index in 0..count {
        let id = u32_at(&data, at).ok_or_else(|| format!("{name}: row {} has no ID", index + 1))?;
        let len = u32_at(&data, at + 4).ok_or_else(|| format!("{name}: row {} has no path length", index + 1))? as usize;
        at += 8;
        if len > MAX_PATH_BYTES {
            return Err(format!("{name}: row {} has a {len}-byte path; the client buffer holds 255 safely", index + 1));
        }
        let raw = data.get(at..at + len).ok_or_else(|| format!("{name}: row {} path runs past the end of the file", index + 1))?;
        let path = GBK
            .decode_without_bom_handling_and_without_replacement(raw)
            .ok_or_else(|| format!("{name}: row {} is not valid GBK", index + 1))?
            .into_owned();
        rows.push(Row { id, path });
        at += len;
    }
    if at != data.len() {
        return Err(format!("{name}: {} trailing bytes follow the declared rows", data.len() - at));
    }
    // Apply the same uniqueness and string rules as the client loader.
    validate(&rows, table)?;
    let size = data.len();
    Ok(FileView { path, size, token: token(&data), rows })
}

pub fn open(path: String) -> Result<FileView, String> {
    open_table(path, Table::Paths)
}

pub fn open_table(path: String, table: Table) -> Result<FileView, String> {
    let data = std::fs::read(&path).map_err(|error| format!("Could not read {path}: {error}"))?;
    parse(path, data, table)
}

pub fn export_json(path: String, source_path: String, rows: Vec<Row>) -> Result<JsonReport, String> {
    export_table_json(path, source_path, rows, Table::Paths)
}

pub fn export_table_json(path: String, source_path: String, rows: Vec<Row>, table: Table) -> Result<JsonReport, String> {
    validate(&rows, table)?;
    let export = JsonExport {
        format: table.json_format(),
        format_version: JSON_FORMAT_VERSION,
        source_path: &source_path,
        exported_at: Local::now().to_rfc3339(),
        records: &rows,
    };
    let contents = serde_json::to_string_pretty(&export).map_err(|error| format!("Could not make the JSON export: {error}"))?;
    std::fs::write(&path, contents).map_err(|error| format!("Could not write {path}: {error}"))?;
    Ok(JsonReport { path, rows: rows.len() })
}

pub fn import_json(path: String) -> Result<JsonImport, String> {
    import_table_json(path, Table::Paths)
}

pub fn import_table_json(path: String, table: Table) -> Result<JsonImport, String> {
    let contents = std::fs::read_to_string(&path).map_err(|error| format!("Could not read {path}: {error}"))?;
    let input: JsonInput = serde_json::from_str(&contents).map_err(|error| format!("Invalid {} JSON: {error}", table.name()))?;
    let (rows, source_path, exported_at) = match input {
        JsonInput::Envelope(envelope) => {
            if envelope.format != table.json_format() {
                return Err(format!("This JSON is “{}”, not a JD IDE {} export", envelope.format, table.name()));
            }
            if envelope.format_version != JSON_FORMAT_VERSION {
                return Err(format!("Path JSON format version {} is not supported", envelope.format_version));
            }
            (envelope.records, envelope.source_path, envelope.exported_at)
        }
        // A plain array is useful for hand-authored tables and older scripts.
        JsonInput::Rows(rows) => (rows, None, None),
    };
    validate(&rows, table)?;
    Ok(JsonImport { rows, source_path, exported_at })
}

pub(crate) fn write_replacing(target: &Path, data: &[u8]) -> Result<(), String> {
    let name = target.file_name().ok_or("The file has no name")?.to_string_lossy().into_owned();
    let temporary = target.with_file_name(format!("{name}.jdide-saving"));
    std::fs::write(&temporary, data).map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    if let Ok(metadata) = std::fs::metadata(target) {
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(target, permissions);
        }
    }
    std::fs::rename(&temporary, target).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("Could not replace {}: {error}", target.display())
    })
}

pub fn save(request: SaveRequest) -> Result<SaveReport, String> {
    save_table(request, Table::Paths)
}

pub fn save_table(request: SaveRequest, table: Table) -> Result<SaveReport, String> {
    let target = PathBuf::from(&request.target_path);
    let opened = Path::new(&request.opened_path);
    if !request.replace_changed && same_path(&target, opened) {
        let current = std::fs::read(&target).map_err(|error| format!("Could not check {}: {error}", target.display()))?;
        if token(&current) != request.token {
            return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", target.display()));
        }
    }
    let data = encode(&request.rows, table)?;
    let backup = if request.backup && target.is_file() {
        Some(crate::backup::archive(&target, std::slice::from_ref(&target))?)
    } else {
        None
    };
    write_replacing(&target, &data)?;
    // Only path.data is part of the elements.data checksum (DynamicObjects.data loads after it).
    let sibling = target.parent().map(|parent| parent.join("elements.data")).filter(|path| table == Table::Paths && path.is_file());
    Ok(SaveReport {
        path: target.display().to_string(),
        size: data.len(),
        rows: request.rows.len(),
        token: token(&data),
        backup: backup.map(|path| path.display().to_string()),
        sibling_elements: sibling.map(|path| path.display().to_string()),
        client_reloaded: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_paths(rows: &[Row]) -> Result<Vec<u8>, String> {
        encode(rows, Table::Paths)
    }

    #[test]
    fn dynamic_object_tables_read_back_byte_for_byte() {
        let paths = [
            "E:/Games/XtremeJade/element/data/dynamicobjects.data",
            "E:/Games/ForsakenJD/element/data/DynamicObjects.data",
            "E:/Games/Elite Jade Dynasty - HDN/element/data/dynamicobjects.data",
        ];
        for path in paths {
            let Ok(data) = std::fs::read(path) else { continue };
            // path.data's stricter rules refuse the official empty path; the dynamic object table takes it.
            assert!(parse(path.into(), data.clone(), Table::Paths).is_err());
            let view = parse(path.into(), data.clone(), Table::DynamicObjects).unwrap();
            assert!(view.rows.iter().any(|row| row.path.is_empty()));
            assert_eq!(encode(&view.rows, Table::DynamicObjects).unwrap(), data, "{path}");
        }
        // Still only one empty path: paths must be unique.
        let row = |id, path: &str| Row { id, path: path.into() };
        assert!(encode(&[row(1, ""), row(2, "")], Table::DynamicObjects).is_err());
    }

    #[test]
    fn round_trips_rows_in_official_id_order() {
        let rows = vec![Row { id: 9, path: "Surfaces\\测试.tga".into() }, Row { id: 2, path: "Models\\sword.ecm".into() }];
        let data = encode(&rows, Table::Paths).unwrap();
        let view = parse("memory/path.data".into(), data.clone(), Table::Paths).unwrap();
        assert_eq!(view.rows.iter().map(|row| row.id).collect::<Vec<_>>(), vec![2, 9]);
        assert_eq!(encode(&view.rows, Table::Paths).unwrap(), data);
    }

    #[test]
    fn rejects_values_the_client_loader_cannot_safely_use() {
        let row = |id, path: &str| Row { id, path: path.into() };
        assert!(encode_paths(&[row(0, "a")]).is_err());
        assert!(encode_paths(&[row(1, "a"), row(1, "b")]).is_err());
        assert!(encode_paths(&[row(1, "a"), row(2, "a")]).is_err());
        assert!(encode_paths(&[row(1, "")]).is_err());
        assert!(encode_paths(&[row(1, &"a".repeat(256))]).is_err());
        assert!(encode_paths(&[row(1, "emoji-😀")]).is_err());
    }

    #[test]
    fn real_path_tables_are_strict_and_byte_exact() {
        let paths = [
            "E:/Games/ForsakenJD/element/data/path.data",
            "E:/Games/Elite Jade Dynasty - HDN/element/data/path.data",
            "E:/Game Dev/JD/zxserver/zgame/gs/config/path.data",
        ];
        for path in paths {
            let Ok(data) = std::fs::read(path) else { continue };
            let view = parse(path.into(), data.clone(), Table::Paths).unwrap();
            assert_eq!(encode(&view.rows, Table::Paths).unwrap(), data);
        }
    }

    #[test]
    fn save_is_atomic_backed_up_and_guarded_against_outside_changes() {
        let dir = std::env::temp_dir().join(format!("jdide-path-data-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("path.data");
        let original = encode_paths(&[Row { id: 1, path: "one.tga".into() }]).unwrap();
        std::fs::write(&path, &original).unwrap();
        let opened = open(path.display().to_string()).unwrap();
        let request = SaveRequest {
            opened_path: opened.path,
            target_path: path.display().to_string(),
            token: opened.token,
            rows: vec![Row { id: 2, path: "two.dds".into() }],
            backup: true,
            replace_changed: false,
        };
        let report = save(request).unwrap();
        assert!(report.backup.as_ref().is_some_and(|backup| Path::new(backup).is_file()));
        assert_eq!(open(path.display().to_string()).unwrap().rows[0].id, 2);

        let stale_token = report.token;
        std::fs::write(&path, &original).unwrap();
        let error = save(SaveRequest {
            opened_path: report.path.clone(),
            target_path: report.path,
            token: stale_token,
            rows: vec![Row { id: 3, path: "three.ecm".into() }],
            backup: false,
            replace_changed: false,
        }).unwrap_err();
        assert!(error.starts_with("CHANGED_ON_DISK:"));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn json_export_import_is_versioned_and_plain_arrays_still_work() {
        let dir = std::env::temp_dir().join(format!("jdide-path-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("paths.json");
        let rows = vec![Row { id: 1, path: "Surfaces\\测试.tga".into() }];
        export_json(path.display().to_string(), "client/path.data".into(), rows.clone()).unwrap();
        let imported = import_json(path.display().to_string()).unwrap();
        assert_eq!(imported.source_path.as_deref(), Some("client/path.data"));
        assert_eq!(imported.rows[0].path, rows[0].path);

        std::fs::write(&path, r#"[{"id":2,"path":"Models\\item.ecm"}]"#).unwrap();
        assert_eq!(import_json(path.display().to_string()).unwrap().rows[0].id, 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
