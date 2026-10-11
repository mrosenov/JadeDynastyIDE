//! DynamicObjects.data: the client's table of models and effects for npcgen.data's dynamic objects (doors, portals,
//! scene effects). Same format and loader as path.data (\`DataPathMan\`, \`path_data::Table::DynamicObjects\`).
//!
//! Client (ZElementClient): \`CECGame::Init\` loads \`data\\DynamicObjects.data\` after elements.data (it is not in the
//! elements checksum); a failure is only logged, and then no dynamic object has a model. \`CECMatter::Init\` takes
//! \`GetDynObjectPath(tid)\` for matters of state 1: an \`.ecm\` path is a model (from the client root, e.g.
//! \`models\\npcs\\…\`), a \`.gfx\` path an effect (below \`gfx\\\`); anything else, or an ID the table lacks (an empty
//! path), shows nothing. The server never reads it: it sends the object's ID (npcgen.data) as the matter type.
//! Written by the map editor's dynamic object manager (ZElementEditor DynamicObjectManDlg::Export, IDs ascending).

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::client::Resources;
use crate::path_data::Row;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectInfo {
    pub id: u32,
    /// \`model\` (.ecm), \`effect\` (.gfx), \`empty\` or \`other\` (the client shows nothing).
    pub kind: &'static str,
    /// The file the client loads (effects below \`gfx\\\`).
    pub file: String,
    /// Whether the configured client has the file (none: no client configured, or nothing to load).
    pub exists: Option<bool>,
}

pub fn kind_of(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if path.trim().is_empty() {
        "empty"
    } else if lower.ends_with(".ecm") {
        "model"
    } else if lower.ends_with(".gfx") {
        "effect"
    } else {
        "other"
    }
}

/// The client-relative file of a table path: effects are loaded from \`gfx\\\`.
pub fn file_of(path: &str) -> String {
    let path = path.replace('/', "\\");
    if kind_of(&path) == "effect" && !path.to_lowercase().starts_with("gfx\\") {
        format!("gfx\\{path}")
    } else {
        path
    }
}

/// Whether the client has a file: in the package named by its first folder, or loose below \`element\\\`.
pub fn client_has(resources: &Resources, file: &str) -> bool {
    let normalized = crate::client::pck::normalize(file);
    let package = normalized.split('\\').next().unwrap_or("");
    if !package.is_empty() && resources.package(package).ok().is_some_and(|pck| pck.find(&normalized).is_some()) {
        return true;
    }
    resources.element_dir().join(file.replace('\\', "/")).is_file()
}

pub fn check(rows: &[Row], resources: Option<&Resources>) -> Vec<ObjectInfo> {
    rows.iter()
        .map(|row| {
            let kind = kind_of(&row.path);
            let file = file_of(&row.path);
            let exists = match (resources, kind) {
                (Some(resources), "model" | "effect") => Some(client_has(resources, &file)),
                _ => None,
            };
            ObjectInfo { id: row.id, kind, file, exists }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// \`error\`: the client refuses the whole table; \`warning\`: an object shows nothing.
    pub severity: &'static str,
    pub id: Option<u32>,
    pub message: String,
}

/// \`used\`: dynamic object IDs of the open npcgen.data with how many objects use each.
pub fn problems(rows: &[Row], infos: &[ObjectInfo], used: &HashMap<u32, usize>) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut ids: HashMap<u32, usize> = HashMap::new();
    let mut paths: HashMap<&str, usize> = HashMap::new();
    for row in rows {
        *ids.entry(row.id).or_default() += 1;
        *paths.entry(row.path.as_str()).or_default() += 1;
    }
    let mut reported = HashSet::new();
    for (row, info) in rows.iter().zip(infos) {
        let mut add = |severity, message: String| out.push(Problem { severity, id: Some(row.id), message });
        if row.id == 0 {
            add("error", "ID 0: the table cannot be saved with it".into());
        }
        if ids[&row.id] > 1 && reported.insert(("id", row.id.to_string())) {
            add("error", format!("ID {} is used {} times: the client refuses the whole table, so no dynamic object gets a model", row.id, ids[&row.id]));
        }
        if paths[row.path.as_str()] > 1 && reported.insert(("path", row.path.clone())) {
            add("error", format!("The path “{}” is used {} times: the client refuses the whole table", row.path, paths[row.path.as_str()]));
        }
        let (bytes, _, unmappable) = encoding_rs::GBK.encode(&row.path);
        if unmappable {
            add("error", "The path has characters GBK cannot store".into());
        } else if bytes.len() > 255 {
            add("error", format!("The path is {} GBK bytes; the client holds at most 255", bytes.len()));
        }
        match info.kind {
            "empty" => add("warning", "No path: objects with this ID show nothing".into()),
            "other" => add("warning", format!("“{}” is neither an .ecm model nor a .gfx effect: the client shows nothing", row.path)),
            _ => {}
        }
        if info.exists == Some(false) {
            add("warning", format!("{} is not in the configured client: the object shows nothing", info.file));
        }
    }
    let known: HashSet<u32> = rows.iter().map(|row| row.id).collect();
    let mut missing: Vec<(&u32, &usize)> = used.iter().filter(|(id, _)| !known.contains(id)).collect();
    missing.sort();
    for (id, count) in missing {
        out.push(Problem { severity: "warning", id: Some(*id), message: format!("The open npcgen.data places {count} dynamic object{} with ID {id}, which this table does not have: {} nothing", if *count == 1 { "" } else { "s" }, if *count == 1 { "it shows" } else { "they show" }) });
    }
    out
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePage {
    /// Paths as the table stores them (effects without \`gfx\\\`).
    pub files: Vec<String>,
    pub total: usize,
}

/// Models (\`models.pck\`, .ecm) or effects (\`gfx.pck\`, .gfx) of the client whose path contains every word of
/// \`query\`, sorted, a page at a time.
pub fn client_files(resources: &Resources, kind: &str, query: &str, offset: usize, limit: usize) -> Result<FilePage, String> {
    let (package, extension, strip) = match kind {
        "model" => ("models", ".ecm", ""),
        "effect" => ("gfx", ".gfx", "gfx\\"),
        other => return Err(format!("Unknown file kind {other}")),
    };
    let pck = resources.package(package)?;
    let words: Vec<String> = query.split_whitespace().map(|word| word.to_lowercase()).collect();
    let mut files: Vec<String> = pck
        .entries
        .iter()
        .filter_map(|entry| {
            let lower = entry.path.to_lowercase();
            if !lower.ends_with(extension) || !words.iter().all(|word| lower.contains(word.as_str())) {
                return None;
            }
            let path = entry.path.replace('/', "\\");
            Some(if !strip.is_empty() && path.to_lowercase().starts_with(strip) { path[strip.len()..].to_string() } else { path })
        })
        .collect();
    files.sort_by_key(|path| path.to_lowercase());
    let total = files.len();
    Ok(FilePage { files: files.into_iter().skip(offset).take(limit).collect(), total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_data::{open_table, Table};

    #[test]
    fn official_tables_point_at_client_files() {
        let clients = [("E:/Games/ForsakenJD", "E:/Games/ForsakenJD/element/data/DynamicObjects.data"), ("E:/Games/Elite Jade Dynasty - HDN", "E:/Games/Elite Jade Dynasty - HDN/element/data/dynamicobjects.data")];
        for (root, table) in clients {
            let Ok(info) = crate::client::inspect(std::path::Path::new(root)) else { continue };
            let resources = Resources::new(info);
            let view = open_table(table.into(), Table::DynamicObjects).unwrap();
            let infos = check(&view.rows, Some(&resources));
            let files = infos.iter().filter(|info| info.exists.is_some()).count();
            let found = infos.iter().filter(|info| info.exists == Some(true)).count();
            let missing: Vec<&str> = infos.iter().filter(|info| info.exists == Some(false)).map(|info| info.file.as_str()).take(8).collect();
            println!("DYNOBJ {root}: {found}/{files} files found; missing {missing:?}");
            assert!(found * 10 >= files * 9, "most official objects exist in the client");
            let problems = problems(&view.rows, &infos, &HashMap::from([(9999, 2)]));
            assert!(problems.iter().all(|problem| problem.severity != "error"));
            assert!(problems.iter().any(|problem| problem.id == Some(37) && problem.message.starts_with("No path")));
            assert!(problems.iter().any(|problem| problem.id == Some(9999)));
            let models = client_files(&resources, "model", "镇魔洞石门", 0, 10).unwrap();
            assert!(models.files.iter().any(|path| path.ends_with("镇魔洞石门.ecm")), "{models:?}");
            let effects = client_files(&resources, "effect", "传送门", 0, 50).unwrap();
            assert!(effects.files.iter().all(|path| !path.to_lowercase().starts_with("gfx\\")));
        }
    }
}
