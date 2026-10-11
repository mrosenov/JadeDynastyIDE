//! domain.data, domain1–4.data and domainlandwar.data: the client's territory maps (guild map, territory war,
//! world map layers, land war). One format for all (ZCommon/globaldataman.cpp `LoadDomainData`, written by
//! `globaldata_save_to`):
//!
//! ```text
//! [u32 "MOXB" marker, newer files]  i32 count
//! count × { WORD name[16]; i32 id, type (0 none, 1–3 class), reward; A3DPOINT2 center (i32 x, y);
//!           i32 n, n × A3DPOINT2 verts; i32 n, n × 3 i32 faces (indexes into verts); i32 n, n × i32 neighbours }
//! i32 n, n × BattleTime { i32 day (0 = Monday), hour, minute }; i32 battles per time slot
//! ```
//!
//! Coordinates are pixels of the map image (DlgTerrWarMap divides them by its scale and draws the triangles).
//! Only the client reads these files; the territory war server (gdeliveryd) reads `domain.sev`, which the client
//! writes from domain1.data and `configs\land.txt` when started with `globalsave`. The client takes domain1's
//! names, types, declaration items and neighbours from `configs\land.txt` (configs.pck).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

const MAGIC: u32 = 0x4258_4f4d;
/// The client's sanity limit for the reader (counts above it are corrupt data).
const MAX_COUNT: usize = 1_000_000;

/// The domain files a client folder has, in the client's load order, with what each one is.
pub const FILES: &[(&str, &str)] = &[
    ("domain.data", "Guild map"),
    ("domain1.data", "Territory war"),
    ("domain2.data", "World map: cross-server"),
    ("domain3.data", "World map: overall"),
    ("domain4.data", "World map: ancient world"),
    ("domainlandwar.data", "Land war"),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Domain {
    pub name: String,
    /// The stored 32-byte name slot, kept while it still says `name` (some slots hold bytes after the end mark).
    #[serde(default)]
    pub name_raw: Vec<u8>,
    pub id: i32,
    /// 0 none, 1–3 first to third class.
    #[serde(rename = "type")]
    pub kind: i32,
    /// Money per week.
    pub reward: i32,
    pub center: [i32; 2],
    pub verts: Vec<[i32; 2]>,
    pub faces: Vec<[i32; 3]>,
    pub neighbours: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainData {
    /// The file starts with the "MOXB" marker (newer clients).
    pub magic: bool,
    pub domains: Vec<Domain>,
    /// Battle times (day 0 = Monday, hour, minute); none when the file ends after the domains.
    pub times: Option<Vec<[i32; 3]>>,
    /// Battles per time slot (the first slot always gets 8).
    pub time_max: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainFile {
    pub path: String,
    /// File name as on disk.
    pub name: String,
    /// What the client uses it for.
    pub label: String,
    pub size: usize,
    /// MD5 of the file as read (changed-on-disk guard).
    pub token: String,
    pub data: DomainData,
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
    fn i32(&mut self, what: &str) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn count(&mut self, what: &str, item: usize) -> Result<usize, String> {
        let at = self.at;
        let count = self.i32(what)?;
        if count < 0 || count as usize > MAX_COUNT || (count as usize).saturating_mul(item) > self.data.len() - self.at {
            return Err(format!("{what}: count {count} at offset {at} does not fit the file"));
        }
        Ok(count as usize)
    }
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|&unit| unit != 0).collect();
    String::from_utf16_lossy(&units)
}

pub fn parse(data: &[u8]) -> Result<DomainData, String> {
    let mut reader = Reader { data, at: 0 };
    let magic = data.len() >= 4 && u32::from_le_bytes(data[..4].try_into().unwrap()) == MAGIC;
    if magic {
        reader.at = 4;
    }
    let count = reader.count("the domain count", 64)?;
    let mut domains = Vec::with_capacity(count);
    for index in 0..count {
        let what = |part: &str| format!("domain {}: {part}", index + 1);
        let name_raw = reader.take(32, &what("name"))?.to_vec();
        let id = reader.i32(&what("ID"))?;
        let kind = reader.i32(&what("type"))?;
        let reward = reader.i32(&what("reward"))?;
        let center = [reader.i32(&what("center"))?, reader.i32(&what("center"))?];
        let verts = (0..reader.count(&what("vertex count"), 8)?).map(|_| Ok([reader.i32(&what("vertex"))?, reader.i32(&what("vertex"))?])).collect::<Result<Vec<_>, String>>()?;
        let faces = (0..reader.count(&what("face count"), 12)?).map(|_| Ok([reader.i32(&what("face"))?, reader.i32(&what("face"))?, reader.i32(&what("face"))?])).collect::<Result<Vec<_>, String>>()?;
        let neighbours = (0..reader.count(&what("neighbour count"), 4)?).map(|_| reader.i32(&what("neighbour"))).collect::<Result<Vec<_>, String>>()?;
        domains.push(Domain { name: utf16(&name_raw), name_raw, id, kind, reward, center, verts, faces, neighbours });
    }
    let (times, time_max) = if reader.at == data.len() {
        (None, 0)
    } else {
        let count = reader.count("the battle time count", 12)?;
        let times = (0..count).map(|_| Ok([reader.i32("a battle time")?, reader.i32("a battle time")?, reader.i32("a battle time")?])).collect::<Result<Vec<_>, String>>()?;
        (Some(times), reader.i32("the battles per time slot")?)
    };
    if reader.at != data.len() {
        return Err(format!("{} bytes follow the battle times", data.len() - reader.at));
    }
    Ok(DomainData { magic, domains, times, time_max })
}

/// The 32-byte name slot: the stored one while it still says the name, else the name with an end mark.
fn name_slot(domain: &Domain) -> Result<Vec<u8>, String> {
    if domain.name_raw.len() == 32 && utf16(&domain.name_raw) == domain.name {
        return Ok(domain.name_raw.clone());
    }
    let units: Vec<u16> = domain.name.encode_utf16().collect();
    if units.len() > 15 {
        return Err(format!("Domain {}: the name has {} characters; at most 15 (the client needs room for the end mark)", domain.id, units.len()));
    }
    let mut slot = vec![0u8; 32];
    for (index, unit) in units.iter().enumerate() {
        slot[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    Ok(slot)
}

pub fn encode(data: &DomainData) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, value: i32| out.extend_from_slice(&value.to_le_bytes());
    if data.magic {
        out.extend_from_slice(&MAGIC.to_le_bytes());
    }
    put(&mut out, data.domains.len() as i32);
    for domain in &data.domains {
        out.extend_from_slice(&name_slot(domain)?);
        for value in [domain.id, domain.kind, domain.reward, domain.center[0], domain.center[1]] {
            put(&mut out, value);
        }
        put(&mut out, domain.verts.len() as i32);
        for vert in &domain.verts {
            put(&mut out, vert[0]);
            put(&mut out, vert[1]);
        }
        put(&mut out, domain.faces.len() as i32);
        for face in &domain.faces {
            for index in face {
                put(&mut out, *index);
            }
        }
        put(&mut out, domain.neighbours.len() as i32);
        for neighbour in &domain.neighbours {
            put(&mut out, *neighbour);
        }
    }
    if let Some(times) = &data.times {
        put(&mut out, times.len() as i32);
        for time in times {
            for value in time {
                put(&mut out, *value);
            }
        }
        put(&mut out, data.time_max);
    }
    Ok(out)
}

fn token(data: &[u8]) -> String {
    format!("{:x}", Md5::digest(data))
}

/// What a file is for, by its name.
pub fn label_of(name: &str) -> String {
    FILES.iter().find(|(file, _)| file.eq_ignore_ascii_case(name)).map_or_else(|| "Domains".to_string(), |(_, label)| label.to_string())
}

pub fn open_file(path: &Path) -> Result<DomainFile, String> {
    let data = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let parsed = parse(&data).map_err(|error| format!("{name}: {error}"))?;
    Ok(DomainFile { path: path.display().to_string(), label: label_of(&name), name, size: data.len(), token: token(&data), data: parsed })
}

/// Every domain file in the folder of `path` (a domain file or the folder), in the client's load order; other
/// `domain*.data` files after them.
pub fn open_folder(path: &Path) -> Result<Vec<DomainFile>, String> {
    let folder = if path.is_dir() { path.to_path_buf() } else { path.parent().map(Path::to_path_buf).unwrap_or_default() };
    let mut found: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map_err(|error| format!("Could not read {}: {error}", folder.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|candidate| candidate.is_file() && candidate.file_name().is_some_and(|name| {
            let name = name.to_string_lossy().to_lowercase();
            name.starts_with("domain") && name.ends_with(".data")
        }))
        .collect();
    let order = |candidate: &PathBuf| {
        let name = candidate.file_name().unwrap().to_string_lossy().to_lowercase();
        (FILES.iter().position(|(file, _)| *file == name).unwrap_or(FILES.len()), name)
    };
    found.sort_by_key(order);
    if found.is_empty() {
        return Err(format!("{} has no domain*.data files", folder.display()));
    }
    found.iter().map(|file| open_file(file)).collect()
}

// ── land.txt / landwar.txt ──

/// A row of `configs\land.txt` (or `landwar.txt`): what the client uses instead of domain1.data's own values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LandRow {
    pub id: i32,
    pub name: String,
    /// 1–3 (anything else: none).
    pub level: i32,
    pub item: i32,
    pub lower: i32,
    pub upper: i32,
    pub neighbours: Vec<i32>,
    /// The name has a space: the client's `sscanf("%d %s %d …")` stops the name there and misreads the rest of the
    /// row (class, items and neighbours become leftovers), so this row does not work in game.
    #[serde(default)]
    pub space_in_name: bool,
}

/// Rows by their tab-separated columns (what the file means; spaces separate columns when a line has no tabs),
/// the header skipped. Names with spaces are marked: the client itself splits at any whitespace.
pub fn parse_land(text: &str) -> Vec<LandRow> {
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = if line.contains('\t') { line.split('\t').map(str::trim).filter(|part| !part.is_empty()).collect() } else { line.split_whitespace().collect() };
            if parts.len() < 6 {
                return None;
            }
            let number = |at: usize| parts.get(at).and_then(|part| part.parse::<i32>().ok());
            Some(LandRow {
                id: number(0)?,
                name: parts[1].to_string(),
                level: number(2)?,
                item: number(3)?,
                lower: number(4)?,
                upper: number(5)?,
                neighbours: parts.get(6).map(|list| list.split(';').filter_map(|id| id.trim().parse().ok()).collect()).unwrap_or_default(),
                space_in_name: parts[1].contains(char::is_whitespace),
            })
        })
        .collect()
}

/// The client's land tables from configs.pck: `land.txt` (domain1.data) and, in newer clients, `landwar.txt`.
pub fn land_tables(resources: &crate::client::Resources) -> HashMap<String, Vec<LandRow>> {
    let mut out = HashMap::new();
    let Ok(pck) = resources.package("configs") else { return out };
    for (file, key) in [("configs\\land.txt", "domain1.data"), ("configs\\landwar.txt", "domainlandwar.data")] {
        if let Ok(data) = pck.read_path(file) {
            let text = if data.starts_with(&[0xff, 0xfe]) { encoding_rs::UTF_16LE.decode(&data).0.into_owned() } else { encoding_rs::GBK.decode(&data).0.into_owned() };
            out.insert(key.to_string(), parse_land(&text));
        }
    }
    out
}

// ── Problems ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// `error`: the client misreads or may crash; `warning`: the map misbehaves; `note`: probably unintended.
    pub severity: &'static str,
    /// The domain's position in the file.
    pub domain: Option<usize>,
    pub message: String,
}

/// Whether a point is inside a polygon (even-odd rule over the outline).
fn inside(point: [i32; 2], verts: &[[i32; 2]]) -> bool {
    let (x, y) = (point[0] as f64, point[1] as f64);
    let mut result = false;
    let mut previous = verts.len().wrapping_sub(1);
    for (index, vert) in verts.iter().enumerate() {
        let (xi, yi) = (vert[0] as f64, vert[1] as f64);
        let (xj, yj) = (verts[previous][0] as f64, verts[previous][1] as f64);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            result = !result;
        }
        previous = index;
    }
    result
}

/// Whether a point lies in one of the triangles (what the client fills).
fn in_triangles(point: [i32; 2], domain: &Domain) -> bool {
    domain.faces.iter().any(|face| {
        let corners: Option<Vec<[i32; 2]>> = face.iter().map(|&index| usize::try_from(index).ok().and_then(|index| domain.verts.get(index).copied())).collect();
        corners.is_some_and(|corners| inside(point, &corners) || corners.contains(&point))
    })
}

pub fn problems(file_name: &str, data: &DomainData, land: Option<&[LandRow]>) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut ids: HashMap<i32, usize> = HashMap::new();
    for domain in &data.domains {
        *ids.entry(domain.id).or_default() += 1;
    }
    let mut reported = HashSet::new();
    for (index, domain) in data.domains.iter().enumerate() {
        let mut add = |severity, message: String| out.push(Problem { severity, domain: Some(index), message });
        if ids[&domain.id] > 1 && reported.insert(domain.id) {
            add("error", format!("ID {} is used by {} domains: the client finds only the first", domain.id, ids[&domain.id]));
        }
        let units = domain.name.encode_utf16().count();
        if units > 15 {
            add("error", format!("The name has {units} characters: the client needs room for the end mark (at most 15)"));
        }
        if !(0..=3).contains(&domain.kind) {
            add("warning", format!("Type {} is not 0–3 (none, first to third class)", domain.kind));
        }
        if let Some(face) = domain.faces.iter().find(|face| face.iter().any(|&index| index < 0 || index as usize >= domain.verts.len())) {
            add("error", format!("A triangle ({}, {}, {}) points past the {} outline points: the client reads outside the list when drawing the area", face[0], face[1], face[2], domain.verts.len()));
        }
        if domain.verts.len() < 3 || domain.faces.is_empty() {
            add("warning", "The domain has no area (fewer than 3 outline points or no triangles): it cannot be seen or clicked on the map".into());
        } else if !in_triangles(domain.center, domain) && !inside(domain.center, &domain.verts) {
            add("note", format!("The center ({}, {}) is outside the area: the faction icon is drawn outside it", domain.center[0], domain.center[1]));
        }
        let mut seen = HashSet::new();
        // 0 is an empty entry (XtremeJade's domain1.data lists only 0: land.txt has the neighbours).
        for &neighbour in domain.neighbours.iter().filter(|&&neighbour| neighbour != 0) {
            if !seen.insert(neighbour) {
                add("note", format!("Neighbour {neighbour} is listed twice"));
            } else if neighbour == domain.id {
                add("warning", "The domain lists itself as a neighbour".into());
            } else if !ids.contains_key(&neighbour) {
                add("warning", format!("Neighbour {neighbour} is not a domain of this file"));
            } else if let Some(other) = data.domains.iter().find(|other| other.id == neighbour) {
                if !other.neighbours.contains(&domain.id) {
                    add("note", format!("Neighbour {neighbour} ({}) does not list this domain back", other.name));
                }
            }
        }
    }
    if let Some(times) = &data.times {
        for (number, time) in times.iter().enumerate() {
            if !(0..=6).contains(&time[0]) || !(0..=23).contains(&time[1]) || !(0..=59).contains(&time[2]) {
                out.push(Problem { severity: "warning", domain: None, message: format!("Battle time {}: day {}, {}:{:02} is not a day 0–6 (Monday–Sunday) and a time of day", number + 1, time[0], time[1], time[2]) });
            }
        }
        if data.time_max < 0 {
            out.push(Problem { severity: "warning", domain: None, message: format!("Battles per time slot is {}", data.time_max) });
        }
    }
    if let Some(land) = land {
        let rows: HashSet<i32> = land.iter().map(|row| row.id).collect();
        let source = if file_name.eq_ignore_ascii_case("domainlandwar.data") { "landwar.txt" } else { "land.txt" };
        for row in land.iter().filter(|row| row.space_in_name) {
            let index = data.domains.iter().position(|domain| domain.id == row.id);
            out.push(Problem { severity: "warning", domain: index, message: format!("configs\\{source}: the name “{}” has a space; the client reads names only up to a space, so it misreads this row's class, declaration items and neighbours", row.name) });
        }
        for (index, domain) in data.domains.iter().enumerate() {
            if !rows.contains(&domain.id) {
                out.push(Problem { severity: "note", domain: Some(index), message: format!("Not in configs\\{source}: the client keeps this file's name and type and has no declaration item for it") });
            }
        }
        let mut missing: Vec<i32> = rows.iter().filter(|id| !ids.contains_key(id)).copied().collect();
        missing.sort_unstable();
        if !missing.is_empty() {
            out.push(Problem { severity: "note", domain: None, message: format!("configs\\{source} has rows for IDs this file lacks: {}", missing.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")) });
        }
    }
    out
}

// ── domain.sev (the territory war server's copy) ──

/// A territory as the server reads it from `domain.sev` (gdeliveryd domaindataman.cpp `domain_data_load`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SevDomain {
    pub id: i32,
    #[serde(rename = "type")]
    pub kind: i32,
    /// Declaration items: the least and the most a guild may bid.
    pub lower: i32,
    pub upper: i32,
    pub neighbours: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sev {
    pub domains: Vec<SevDomain>,
    pub times: Vec<[i32; 3]>,
    pub time_max: i32,
}

/// What the client's `globalsave` writes (globaldataman.cpp): domain1.data's territories after `configs\land.txt`
/// replaced their type (level 1–3, else 0), item limits and neighbours (the last row of an ID wins); a territory
/// without a row keeps the file's type and neighbours and has no item limits. `time_max`: the official client
/// writes 0 (its `nTimeMax` is never set); both servers replace the times and the maximum with fixed ones anyway.
pub fn sev_from(data: &DomainData, land: Option<&[LandRow]>, time_max: i32) -> Sev {
    let rows: HashMap<i32, &LandRow> = land.unwrap_or_default().iter().map(|row| (row.id, row)).collect();
    let mut seen = HashSet::new();
    let domains = data
        .domains
        .iter()
        .map(|domain| {
            // The client applies a row to the first domain of its ID only.
            let row = if seen.insert(domain.id) { rows.get(&domain.id) } else { None };
            match row {
                Some(row) => SevDomain { id: domain.id, kind: if (1..=3).contains(&row.level) { row.level } else { 0 }, lower: row.lower, upper: row.upper, neighbours: row.neighbours.clone() },
                None => SevDomain { id: domain.id, kind: domain.kind, lower: 0, upper: 0, neighbours: domain.neighbours.clone() },
            }
        })
        .collect();
    Sev { domains, times: data.times.clone().unwrap_or_default(), time_max }
}

pub fn encode_sev(sev: &Sev) -> Vec<u8> {
    let mut out = Vec::new();
    let mut put = |value: i32| out.extend_from_slice(&value.to_le_bytes());
    put(sev.domains.len() as i32);
    for domain in &sev.domains {
        for value in [domain.id, domain.kind, domain.lower, domain.upper, domain.neighbours.len() as i32] {
            put(value);
        }
        for &neighbour in &domain.neighbours {
            put(neighbour);
        }
    }
    put(sev.times.len() as i32);
    for time in &sev.times {
        for &value in time {
            put(value);
        }
    }
    put(sev.time_max);
    out
}

pub fn parse_sev(data: &[u8]) -> Result<Sev, String> {
    let mut reader = Reader { data, at: 0 };
    let count = reader.count("the territory count", 20)?;
    let mut domains = Vec::with_capacity(count);
    for index in 0..count {
        let what = format!("territory {}", index + 1);
        let (id, kind, lower, upper) = (reader.i32(&what)?, reader.i32(&what)?, reader.i32(&what)?, reader.i32(&what)?);
        let neighbours = (0..reader.count(&what, 4)?).map(|_| reader.i32(&what)).collect::<Result<Vec<_>, String>>()?;
        domains.push(SevDomain { id, kind, lower, upper, neighbours });
    }
    let times = (0..reader.count("the battle time count", 12)?).map(|_| Ok([reader.i32("a battle time")?, reader.i32("a battle time")?, reader.i32("a battle time")?])).collect::<Result<Vec<_>, String>>()?;
    let time_max = reader.i32("the battles per time slot")?;
    if reader.at != data.len() {
        return Err(format!("{} bytes follow the battle times", data.len() - reader.at));
    }
    Ok(Sev { domains, times, time_max })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SevPreview {
    pub sev: Sev,
    /// The target file as it is now (none: it does not exist or is not a domain.sev).
    pub existing: Option<Sev>,
    /// Why the target could not be read.
    pub existing_error: Option<String>,
    /// The export would write exactly the target's bytes.
    pub same: bool,
}

pub fn sev_preview(data: &DomainData, land: Option<&[LandRow]>, time_max: i32, target: Option<&Path>) -> SevPreview {
    let sev = sev_from(data, land, time_max);
    let (existing, existing_error, same) = match target.map(std::fs::read) {
        Some(Ok(bytes)) => match parse_sev(&bytes) {
            Ok(existing) => (Some(existing), None, encode_sev(&sev) == bytes),
            Err(error) => (None, Some(error), false),
        },
        _ => (None, None, false),
    };
    SevPreview { sev, existing, existing_error, same }
}

/// Writes domain.sev (atomic, 7z backup of the replaced file); the bytes must read back as the territories.
pub fn export_sev(sev: &Sev, target: &Path, backup: bool) -> Result<SaveReport, String> {
    let data = encode_sev(sev);
    if parse_sev(&data)? != *sev {
        return Err("The written domain.sev would not read back the same; nothing was written".into());
    }
    let backup = if backup && target.is_file() { Some(crate::backup::archive(target, &[target.to_path_buf()])?.display().to_string()) } else { None };
    crate::path_data::write_replacing(target, &data)?;
    Ok(SaveReport { files: vec![(target.display().to_string(), data.len(), token(&data))], backup })
}

// ── Saving ──

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFile {
    pub path: String,
    /// The token the file had when opened (none: a new target, e.g. Save as).
    #[serde(default)]
    pub token: Option<String>,
    pub data: DomainData,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReport {
    /// Per file: path, size, new token.
    pub files: Vec<(String, usize, String)>,
    pub backup: Option<String>,
}

/// Writes the files after checking that each reads back as written and that none was changed by another program
/// since it was read (unless `replace_changed`). One 7z backup holds every replaced file.
pub fn save(files: &[SaveFile], backup: bool, replace_changed: bool) -> Result<SaveReport, String> {
    let mut encoded = Vec::with_capacity(files.len());
    for file in files {
        let path = PathBuf::from(&file.path);
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let data = encode(&file.data).map_err(|error| format!("{name}: {error}"))?;
        // Read back, the name slots are the written ones.
        let expected = DomainData { domains: file.data.domains.iter().map(|domain| Domain { name_raw: name_slot(domain).unwrap_or_default(), ..domain.clone() }).collect(), ..file.data.clone() };
        if parse(&data)? != expected {
            return Err(format!("{name}: the written file would not read back the same; nothing was saved"));
        }
        if !replace_changed {
            if let (Some(expected), Ok(current)) = (&file.token, std::fs::read(&path)) {
                if &token(&current) != expected {
                    return Err(format!("CHANGED_ON_DISK: {} was changed by another program since it was opened", path.display()));
                }
            }
        }
        encoded.push((path, data));
    }
    let existing: Vec<PathBuf> = encoded.iter().map(|(path, _)| path.clone()).filter(|path| path.is_file()).collect();
    let backup = if backup && !existing.is_empty() { Some(crate::backup::archive(&existing[0], &existing)?.display().to_string()) } else { None };
    let mut report = Vec::new();
    for (path, data) in &encoded {
        crate::path_data::write_replacing(path, data)?;
        report.push((path.display().to_string(), data.len(), token(data)));
    }
    Ok(SaveReport { files: report, backup })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders() -> Vec<&'static str> {
        vec![
            "E:/Games/XtremeJade/element/data",
            "E:/Games/ForsakenJD/element/data",
            "E:/Games/Elite Jade Dynasty - HDN/element/data",
            "E:/Game Dev/JD/zxserver/zgame/gs/config",
            "E:/Game Dev/JD/1559/gamed/config",
        ]
    }

    #[test]
    fn every_sample_reads_back_byte_for_byte() {
        let mut checked = 0;
        for folder in folders() {
            let Ok(files) = open_folder(Path::new(folder)) else { continue };
            for file in files {
                let original = std::fs::read(&file.path).unwrap();
                assert_eq!(encode(&file.data).unwrap(), original, "{}", file.path);
                let problems = problems(&file.name, &file.data, None);
                let errors: Vec<&Problem> = problems.iter().filter(|problem| problem.severity == "error").collect();
                println!("DOMAIN {} ({}): {} domains, {} problems, {} errors {:?}", file.path, file.label, file.data.domains.len(), problems.len(), errors.len(), problems.iter().take(2).map(|problem| &problem.message).collect::<Vec<_>>());
                assert!(errors.is_empty(), "official files load in the client");
                checked += 1;
            }
        }
        println!("DOMAIN files checked: {checked}");
    }

    #[test]
    fn edits_are_checked_and_written() {
        let Some(file) = folders().into_iter().find_map(|folder| open_folder(Path::new(folder)).ok()).and_then(|files| files.into_iter().find(|file| file.name.eq_ignore_ascii_case("domain1.data"))) else { return };
        let mut data = file.data.clone();
        data.domains[0].name = "A new name".into();
        data.domains[0].reward = 1234;
        let written = encode(&data).unwrap();
        let back = parse(&written).unwrap();
        assert_eq!(back.domains[0].name, "A new name");
        assert_eq!(back.domains[0].reward, 1234);
        assert_eq!(back.domains[1], file.data.domains[1]);
        data.domains[0].name = "Sixteen letters!".into();
        assert!(encode(&data).is_err());
        // Problems: a missing neighbour and a triangle past the outline.
        data.domains[0].name = "Ok".into();
        data.domains[0].neighbours.push(99_999);
        data.domains[1].faces.push([0, 1, 10_000]);
        let found = problems(&file.name, &data, None);
        assert!(found.iter().any(|problem| problem.domain == Some(0) && problem.message.contains("99999")));
        assert!(found.iter().any(|problem| problem.domain == Some(1) && problem.severity == "error"));
        // Save into a temporary copy: backup, read back, changed-on-disk guard.
        let dir = std::env::temp_dir().join(format!("jdide-domains-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("domain1.data");
        std::fs::write(&target, std::fs::read(&file.path).unwrap()).unwrap();
        let opened = open_file(&target).unwrap();
        let mut edited = opened.data.clone();
        edited.domains[2].reward = 77;
        let report = save(&[SaveFile { path: target.display().to_string(), token: Some(opened.token.clone()), data: edited }], true, false).unwrap();
        assert!(report.backup.as_ref().is_some_and(|path| Path::new(path).is_file()));
        assert_eq!(open_file(&target).unwrap().data.domains[2].reward, 77);
        let stale = save(&[SaveFile { path: target.display().to_string(), token: Some(opened.token), data: opened.data }], false, false).unwrap_err();
        assert!(stale.starts_with("CHANGED_ON_DISK:"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn domain_sev_matches_the_official_export() {
        // Each server's domain.sev against the client it belongs to: domain1.data + the client's land.txt.
        let pairs = [
            ("E:/Games/XtremeJade", "E:/Game Dev/JD/zxserver/znet/gdeliveryd/domain.sev"),
            ("E:/Games/ForsakenJD", "E:/Game Dev/JD/1559/gdeliveryd/domain.sev"),
        ];
        for (client, sev_path) in pairs {
            let (Ok(info), Ok(official)) = (crate::client::inspect(Path::new(client)), std::fs::read(sev_path)) else { continue };
            let resources = crate::client::Resources::new(info);
            let land = land_tables(&resources);
            let domain1 = open_file(&Path::new(client).join("element/data/domain1.data")).unwrap();
            let sev = sev_from(&domain1.data, land.get("domain1.data").map(Vec::as_slice), 0);
            let ours = encode_sev(&sev);
            let parsed = parse_sev(&official).unwrap();
            let differing: Vec<i32> = sev.domains.iter().zip(&parsed.domains).filter(|(a, b)| a != b).map(|(a, _)| a.id).collect();
            println!("SEV {client} vs {sev_path}: {} territories, same bytes {}, differing {differing:?}, times same {}", sev.domains.len(), ours == official, sev.times == parsed.times);
            assert!(ours == official, "the export equals the official globalsave output");
            assert_eq!(encode_sev(&parsed), official, "domain.sev reads back byte for byte");
            assert_eq!(sev.domains.len(), parsed.domains.len());
        }
    }

    #[test]
    fn land_rows_read_like_the_client() {
        let rows = parse_land("区域id\t区域名称\t区域等级\t宣战品id\t下限\t上限\t相邻区域id\n1\tFort\t1\t19680\t100\t1000\t16;20;23\n2 Royal 2 19680 50 1000 30\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].neighbours, vec![16, 20, 23]);
        assert_eq!(rows[1].level, 2);
    }
}
