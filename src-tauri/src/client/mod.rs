//! The game client folder: its data files, resource packages (`.pck`), the
//! path table (`path.data`) and the item icon atlas.

pub mod dds;
pub mod instances;
pub mod pck;
pub mod strings;
pub mod tga;
pub mod titles;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use encoding_rs::GBK;
use serde::Serialize;

use dds::Dds;
use pck::Pck;

// ---------------------------------------------------------------- folder detection

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// What the file holds, e.g. "elements", "tasks", "gshop".
    pub kind: String,
    /// The app can open this kind of file yet.
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFile {
    pub name: String,
    pub path: String,
    /// Bytes across the .pck and its .pkx parts.
    pub size: u64,
    pub parts: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// The folder the user picked.
    pub root: String,
    /// The `element` folder (holding `data/` and the packages).
    pub element_dir: String,
    pub data_files: Vec<DataFile>,
    pub packages: Vec<PackageFile>,
    pub elements_path: Option<String>,
    pub has_path_data: bool,
    pub has_item_icons: bool,
}

const ITEM_ICONS: &str = "surfaces\\iconset\\iconlist_ivtr";
const IMAGE_CACHE: usize = 32;

fn image_content_type(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "tga" | "dds" | "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

/// Kind of a client data file by name ("tasks.data12" → "tasks").
fn data_kind(name: &str) -> String {
    let lower = name.to_lowercase();
    let stem = lower.split(".data").next().unwrap_or(&lower);
    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    match stem {
        "elements" => "elements",
        "tasks" | "dyn_tasks" => "tasks",
        "gshop" => "gshop",
        "npcgen" => "npcgen",
        "path" => "path",
        s if s.starts_with("domain") => "domain",
        "task_npc" => "task npc",
        "dynamicobjects" => "dynamic objects",
        _ => "other",
    }
    .to_string()
}

/// The `element` folder for a picked folder: the client root, or `element` itself.
pub fn element_dir(picked: &Path) -> Option<PathBuf> {
    [picked.join("element"), picked.to_path_buf()].into_iter().find(|d| d.join("data").is_dir())
}

pub fn inspect(picked: &Path) -> Result<ClientInfo, String> {
    if !picked.is_dir() {
        return Err(format!("{} is not a folder", picked.display()));
    }
    let element = element_dir(picked)
        .ok_or_else(|| format!("No element\\data folder in {}. Pick the game client folder.", picked.display()))?;

    let mut data_files: Vec<DataFile> = std::fs::read_dir(element.join("data"))
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.to_lowercase().contains(".data").then(|| {
                let kind = data_kind(&name);
                DataFile {
                    supported: kind == "elements" || (kind == "tasks" && name.eq_ignore_ascii_case("tasks.data")),
                    path: e.path().display().to_string(),
                    size: e.metadata().map(|m| m.len()).unwrap_or(0),
                    name,
                    kind,
                }
            })
        })
        .collect();
    data_files.sort_by(|a, b| (a.kind != "elements", &a.kind, &a.name).cmp(&(b.kind != "elements", &b.kind, &b.name)));

    let mut packages: Vec<PackageFile> = std::fs::read_dir(&element)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("pck")))
        .map(|e| {
            let path = e.path();
            let mut size = e.metadata().map(|m| m.len()).unwrap_or(0);
            let mut parts = 1;
            for n in 0.. {
                let ext = if n == 0 { "pkx".to_string() } else { format!("pkx{n}") };
                let Ok(meta) = std::fs::metadata(path.with_extension(ext)) else { break };
                size += meta.len();
                parts += 1;
            }
            PackageFile { name: e.file_name().to_string_lossy().into_owned(), path: path.display().to_string(), size, parts }
        })
        .collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    let elements_path = data_files.iter().find(|f| f.name.eq_ignore_ascii_case("elements.data")).map(|f| f.path.clone());
    Ok(ClientInfo {
        root: picked.display().to_string(),
        element_dir: element.display().to_string(),
        has_path_data: element.join("data/path.data").is_file(),
        has_item_icons: element.join("surfaces.pck").is_file(),
        data_files,
        packages,
        elements_path,
    })
}

// ---------------------------------------------------------------- path.data

/// `path.data`: path IDs used by data files → resource paths.
///
/// ```text
/// u32 magic 0x504D4944 ("PMID"), u32 count, count × { u32 id, u32 len, len bytes (GBK) }
/// ```
pub struct PathTable {
    paths: HashMap<u32, String>,
}

impl PathTable {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        if u32_at(0) != Some(0x504d_4944) {
            return Err("path.data: not a path table".into());
        }
        let count = u32_at(4).unwrap_or(0) as usize;
        let mut paths = HashMap::with_capacity(count);
        let mut p = 8;
        for _ in 0..count {
            let (Some(id), Some(len)) = (u32_at(p), u32_at(p + 4)) else { break };
            let Some(raw) = data.get(p + 8..p + 8 + len as usize) else { break };
            paths.insert(id, GBK.decode(raw).0.into_owned());
            p += 8 + len as usize;
        }
        Ok(Self { paths })
    }

    pub fn get(&self, id: u32) -> Option<&str> {
        self.paths.get(&id).map(String::as_str)
    }

    fn iter(&self) -> impl Iterator<Item = (u32, &str)> {
        self.paths.iter().map(|(&id, path)| (id, path.as_str()))
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.paths.len()
    }
}

// ---------------------------------------------------------------- icon sets

/// An icon atlas: `<name>.dds` with `<name>.txt` listing icon size, grid and
/// the file name of each cell, left to right, top to bottom.
pub struct IconSet {
    atlas: Dds,
    pub icon_w: usize,
    pub icon_h: usize,
    columns: usize,
    /// Lowercased icon file name → cell index.
    index: HashMap<String, usize>,
    cache: Mutex<HashMap<usize, Arc<Vec<u8>>>>,
}

impl IconSet {
    pub fn parse(list: &[u8], atlas: Vec<u8>) -> Result<Self, String> {
        let text = GBK.decode(list).0;
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        let mut num = |what: &str| {
            lines
                .next()
                .and_then(|l| l.parse::<usize>().ok())
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("icon list: bad {what}"))
        };
        let (icon_w, icon_h, _rows, columns) = (num("width")?, num("height")?, num("rows")?, num("columns")?);
        let mut index = HashMap::new();
        for (i, name) in lines.enumerate() {
            index.entry(name.to_lowercase()).or_insert(i);
        }
        Ok(Self { atlas: Dds::parse(atlas)?, icon_w, icon_h, columns, index, cache: Mutex::new(HashMap::new()) })
    }

    /// Cell of an icon by any path ending in its file name.
    pub fn find(&self, path: &str) -> Option<usize> {
        let name = path.rsplit(['\\', '/']).next()?.to_lowercase();
        self.index.get(&name).copied()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// One icon as a PNG, decoded from the atlas on first use.
    pub fn png(&self, cell: usize) -> Result<Arc<Vec<u8>>, String> {
        if let Some(hit) = self.cache.lock().map_err(|_| "icon cache poisoned")?.get(&cell) {
            return Ok(hit.clone());
        }
        let (x, y) = ((cell % self.columns) * self.icon_w, (cell / self.columns) * self.icon_h);
        let rgba = self.atlas.rect(x, y, self.icon_w, self.icon_h);
        let png = Arc::new(dds::png(&rgba, self.icon_w, self.icon_h)?);
        self.cache.lock().map_err(|_| "icon cache poisoned")?.insert(cell, png.clone());
        Ok(png)
    }
}

// ---------------------------------------------------------------- resources

/// Client resources, loaded on first use.
pub struct Resources {
    element: PathBuf,
    paths: OnceLock<Result<PathTable, String>>,
    item_icons: OnceLock<Result<IconSet, String>>,
    packages: Mutex<HashMap<String, Arc<Pck>>>,
    /// String tables of configs.pck by file name, read on first use.
    tables: Mutex<HashMap<&'static str, Arc<Result<strings::StringTable, String>>>>,
    item_colors: OnceLock<Option<strings::ItemColors>>,
    /// Standalone image resources, converted or passed through on first use.
    images: Mutex<VecDeque<(u32, Arc<ResourceImage>)>>,
    /// Title definitions from interfaces.pck, read on first use.
    titles: OnceLock<Result<titles::TitleTable, String>>,
    /// Map names from configs.pck instance.txt, read on first use.
    instances: OnceLock<Result<Vec<(i32, String)>, String>>,
}

pub struct ResourceImage {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

#[derive(Debug)]
pub(crate) struct ResourceChoice {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub icon: bool,
}

/// The string tables of configs.pck the editor reads.
pub mod table {
    pub const ITEM_DESC: &str = "item_ext_desc.txt";
    pub const ITEM_TEXTS: &str = "item_desc.txt";
    pub const SKILLS: &str = "skillstr.txt";
    pub const BUFFS: &str = "buff_str.txt";
    pub const ADDONS: &str = "addon_str.txt";
    pub const MONSTERS: &str = "monster_desc.txt";
}

impl Resources {
    pub fn new(info: ClientInfo) -> Self {
        Self {
            element: PathBuf::from(&info.element_dir),
            paths: OnceLock::new(),
            item_icons: OnceLock::new(),
            packages: Mutex::new(HashMap::new()),
            tables: Mutex::new(HashMap::new()),
            item_colors: OnceLock::new(),
            images: Mutex::new(VecDeque::new()),
            titles: OnceLock::new(),
            instances: OnceLock::new(),
        }
    }

    /// A package of the client, e.g. "surfaces", opened once.
    pub fn package(&self, name: &str) -> Result<Arc<Pck>, String> {
        let key = name.to_lowercase();
        let mut open = self.packages.lock().map_err(|_| "package lock poisoned")?;
        if let Some(p) = open.get(&key) {
            return Ok(p.clone());
        }
        let pck = Arc::new(Pck::open(&self.element.join(format!("{key}.pck")))?);
        open.insert(key, pck.clone());
        Ok(pck)
    }

    /// The client's path.data (also part of the elements.data digest).
    pub fn path_data_file(&self) -> PathBuf {
        self.element.join("data/path.data")
    }

    pub fn paths(&self) -> Result<&PathTable, String> {
        self.paths
            .get_or_init(|| {
                let data = std::fs::read(self.element.join("data/path.data")).map_err(|e| format!("path.data: {e}"))?;
                PathTable::parse(&data)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn path(&self, id: u32) -> Option<&str> {
        self.paths().ok()?.get(id)
    }

    pub fn item_icons(&self) -> Result<&IconSet, String> {
        self.item_icons
            .get_or_init(|| {
                let surfaces = self.package("surfaces")?;
                let list = surfaces.read_path(&format!("{ITEM_ICONS}.txt"))?;
                let atlas = surfaces.read_path(&format!("{ITEM_ICONS}.dds"))?;
                IconSet::parse(&list, atlas)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Map names by ID (configs.pck Configs/instance.txt), in file order.
    pub fn instances(&self) -> Result<&Vec<(i32, String)>, String> {
        self.instances
            .get_or_init(|| self.package("configs").and_then(|pck| pck.read_path("configs/instance.txt")).and_then(|bytes| instances::parse(&bytes)))
            .as_ref()
            .map_err(Clone::clone)
    }

    /// A string table of configs.pck (see [`table`]), read once.
    pub fn table(&self, name: &'static str) -> Arc<Result<strings::StringTable, String>> {
        if let Some(t) = self.tables.lock().ok().and_then(|t| t.get(name).cloned()) {
            return t;
        }
        // Read outside the lock: item_ext_desc.txt is several MB.
        let loaded = Arc::new(self.package("configs").and_then(|p| p.read_path(&format!("configs/{name}"))).and_then(|b| strings::StringTable::parse(&b)).map_err(|e| format!("{name}: {e}")));
        if let Ok(mut t) = self.tables.lock() {
            t.entry(name).or_insert(loaded).clone()
        } else {
            loaded
        }
    }

    /// Entry `n` of a string table, when the table and the entry exist.
    pub fn text(&self, name: &'static str, n: u32) -> Option<String> {
        self.table(name).as_ref().as_ref().ok()?.get(n).map(str::to_string)
    }

    /// A skill's name: entry id × 10 of skillstr.txt.
    pub fn skill_name(&self, id: u32) -> Option<String> {
        strings::first_line(&self.text(table::SKILLS, id.checked_mul(10)?)?)
    }

    /// A skill's introduction and detailed description: entries id × 10 + 1
    /// and +2 of skillstr.txt. Some internal skills have only the latter.
    /// Format placeholders remain because an elements.data reference has no level.
    pub fn skill_description(&self, id: u32) -> Option<String> {
        let base = id.checked_mul(10)?;
        let mut parts = [base.checked_add(1)?, base.checked_add(2)?]
            .into_iter()
            .filter_map(|entry| self.text(table::SKILLS, entry))
            .filter_map(|text| strings::colored_text(&text))
            .map(|text| text.replace("%%", "%"));
        let first = parts.next()?;
        Some(parts.fold(first, |mut description, part| {
            if part != description {
                description.push_str("\n\n");
                description.push_str(&part);
            }
            description
        }))
    }

    /// A buff's name: the first line of its buff_str.txt entry.
    pub fn buff_name(&self, id: u32) -> Option<String> {
        strings::first_line(&self.text(table::BUFFS, id)?)
    }

    /// A buff's game-formatted name and description from buff_str.txt.
    pub fn buff_description(&self, id: u32) -> Option<String> {
        strings::colored_text(&self.text(table::BUFFS, id)?)
    }

    fn titles(&self) -> Result<&titles::TitleTable, String> {
        self.titles
            .get_or_init(|| {
                let bytes = self.package("interfaces")?.read_path("interfaces/script/config/title_def_u.lua")?;
                titles::TitleTable::parse(&bytes)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// A title's display name, without its game colour code.
    pub fn title_name(&self, id: u32) -> Option<String> {
        strings::first_line(&self.titles().ok()?.get(id)?.name)
    }

    /// A title and its description, retaining game colours for the popover.
    pub fn title_description(&self, id: u32) -> Option<String> {
        let title = self.titles().ok()?.get(id)?;
        let description = strings::colored_text(&title.description)?;
        Some(format!("{}\n{}", title.name.trim(), description))
    }

    /// An item's name colour (item_color.txt), when it is not white.
    pub fn item_color(&self, id: u32) -> Option<&str> {
        self.item_colors
            .get_or_init(|| {
                let bytes = self.package("configs").ok()?.read_path("configs/item_color.txt").ok()?;
                let desc = self.table(table::ITEM_TEXTS);
                Some(strings::ItemColors::parse(&bytes, desc.as_ref().as_ref().ok()))
            })
            .as_ref()?
            .get(id)
    }

    /// The item icon cell for a path ID (an item's `file_icon`).
    pub fn item_icon(&self, path_id: u32) -> Option<usize> {
        let path = self.path(path_id)?;
        self.item_icons().ok()?.find(path)
    }

    /// Searches one of the numeric resources used by a display role. Tables
    /// stay lazy and only the bounded matches are enriched with descriptions.
    pub(crate) fn search_choices(&self, role: &str, query: &str, current: Option<u32>, offset: usize, limit: usize) -> Result<(Vec<ResourceChoice>, usize), String> {
        let query = query.trim();
        let numeric = query.parse::<u32>().ok();
        let needle = query.to_lowercase();
        let rank = |id: u32, name: &str| {
            if numeric == Some(id) || (query.is_empty() && current == Some(id)) {
                Some(0)
            } else if query.is_empty() {
                Some(4)
            } else {
                let lower = name.to_lowercase();
                if lower == needle { Some(1) } else if lower.starts_with(&needle) { Some(2) } else if lower.contains(&needle) { Some(3) } else { None }
            }
        };

        let mut found: Vec<(u8, u32, String)> = match role {
            "skill" => {
                let table = self.table(table::SKILLS);
                let table = table.as_ref().as_ref().map_err(Clone::clone)?;
                table.strings.iter().filter_map(|(&entry, text)| {
                    (entry > 0 && entry % 10 == 0).then_some((entry / 10, text)).and_then(|(id, text)| {
                        let name = strings::first_line(text)?;
                        Some((rank(id, &name)?, id, name))
                    })
                }).collect()
            }
            "buff" => {
                let table = self.table(table::BUFFS);
                let table = table.as_ref().as_ref().map_err(Clone::clone)?;
                table.strings.iter().filter_map(|(&id, text)| {
                    if id == 0 { return None; }
                    let name = strings::first_line(text)?;
                    Some((rank(id, &name)?, id, name))
                }).collect()
            }
            "title" => self.titles()?.iter().filter_map(|(id, title)| {
                if id == 0 { return None; }
                let name = strings::first_line(&title.name)?;
                Some((rank(id, &name)?, id, name))
            }).collect(),
            "path" | "icon" | "image" => self.paths()?.iter().filter_map(|(id, path)| {
                if id == 0 { return None; }
                if role == "image" && !self.has_image(id) { return None; }
                Some((rank(id, path)?, id, path.to_string()))
            }).collect(),
            _ => return Err(format!("{role} does not have a value picker")),
        };
        found.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let total = found.len();
        let choices = found.into_iter().skip(offset).take(limit).map(|(_, id, name)| {
            let description = match role {
                "skill" => self.skill_description(id),
                "buff" => self.buff_description(id),
                "title" => self.title_description(id),
                _ => None,
            };
            ResourceChoice { id, name, description, icon: role == "icon" && self.item_icon(id).is_some() }
        }).collect();
        Ok((choices, total))
    }

    /// The item icon for a path ID as a PNG.
    pub fn item_icon_png(&self, path_id: u32) -> Result<Arc<Vec<u8>>, String> {
        let cell = self.item_icon(path_id).ok_or_else(|| format!("no icon for path {path_id}"))?;
        self.item_icons()?.png(cell)
    }

    /// A standalone image named by path.data, read from the package named by
    /// the path's first component. TGA and DDS are converted to PNG; formats
    /// the webview understands are passed through unchanged.
    pub fn image(&self, path_id: u32) -> Result<Arc<ResourceImage>, String> {
        if let Some((_, hit)) = self.images.lock().map_err(|_| "image cache poisoned")?.iter().find(|(id, _)| *id == path_id) {
            return Ok(hit.clone());
        }
        let path = self.path(path_id).ok_or_else(|| format!("path {path_id} is not in path.data"))?;
        let content_type = image_content_type(path).ok_or_else(|| format!("{path} is not a supported image"))?;
        let package_name = path.split(['\\', '/']).next().filter(|name| !name.is_empty()).ok_or("resource path has no package name")?;
        let bytes = self.package(package_name)?.read_path(path)?;
        let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let image = match extension.as_str() {
            "tga" => {
                let (rgba, width, height) = tga::decode(&bytes)?;
                ResourceImage { bytes: dds::png(&rgba, width, height)?, content_type: "image/png" }
            }
            "dds" => {
                let image = Dds::parse(bytes)?;
                let rgba = image.rect(0, 0, image.width, image.height);
                ResourceImage { bytes: dds::png(&rgba, image.width, image.height)?, content_type: "image/png" }
            }
            _ => ResourceImage { bytes, content_type },
        };
        let image = Arc::new(image);
        let mut cache = self.images.lock().map_err(|_| "image cache poisoned")?;
        cache.retain(|(id, _)| *id != path_id);
        cache.push_back((path_id, image.clone()));
        while cache.len() > IMAGE_CACHE {
            cache.pop_front();
        }
        Ok(image)
    }

    pub fn has_image(&self, path_id: u32) -> bool {
        let Some(path) = self.path(path_id).filter(|path| image_content_type(path).is_some()) else { return false };
        let Some(package_name) = path.split(['\\', '/']).next().filter(|name| !name.is_empty()) else { return false };
        self.package(package_name).ok().is_some_and(|package| package.find(path).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Option<PathBuf> {
        let dir = PathBuf::from(std::env::var("JDIDE_CLIENT").unwrap_or_else(|_| "E:/Games/ForsakenJD".into()));
        dir.join("element/data/elements.data").exists().then_some(dir)
    }

    #[test]
    fn inspects_a_client_folder() {
        let Some(dir) = client() else { return eprintln!("skipping: no client found") };
        // The client root and its element folder both work.
        for picked in [dir.clone(), dir.join("element")] {
            let info = inspect(&picked).unwrap();
            assert!(info.elements_path.as_deref().is_some_and(|p| p.ends_with("elements.data")));
            assert_eq!(info.data_files[0].kind, "elements");
            assert!(info.data_files.iter().any(|f| f.kind == "tasks" && !f.supported));
            assert!(info.packages.iter().any(|p| p.name.eq_ignore_ascii_case("surfaces.pck")));
            assert!(info.has_path_data && info.has_item_icons);
        }
        assert!(inspect(&std::env::temp_dir()).is_err());
    }

    #[test]
    fn resolves_item_icons_through_path_data() {
        let Some(dir) = client() else { return };
        let res = Resources::new(inspect(&dir).unwrap());
        assert!(res.paths().unwrap().len() > 10_000);
        let icons = res.item_icons().unwrap();
        assert_eq!((icons.icon_w, icons.icon_h), (36, 36));
        assert!(icons.len() > 10_000);
        // The first path whose file name is in the item atlas gives a 36×36 PNG.
        let id = (1..200_000).find(|&id| res.item_icon(id).is_some()).expect("an item icon path");
        let png = res.item_icon_png(id).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let decoder = png::Decoder::new(&png[..]);
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (36, 36));
    }

    #[test]
    fn previews_hdn_npc_profile_tga() {
        let dir = PathBuf::from("E:/Games/Elite Jade Dynasty - HDN");
        if !dir.join("element/data/path.data").exists() || !dir.join("element/surfaces.pck").exists() {
            return eprintln!("skipping: HDN client not found");
        }
        let resources = Resources::new(inspect(&dir).unwrap());
        let path = resources.path(7318).expect("HDN path 7318");
        assert!(path.to_lowercase().starts_with("surfaces\\npcimg\\"), "{path}");
        assert!(path.to_lowercase().ends_with(".tga"), "{path}");
        let image = resources.image(7318).unwrap();
        assert_eq!(image.content_type, "image/png");
        assert_eq!(&image.bytes[..8], b"\x89PNG\r\n\x1a\n");

        let (choices, total) = resources.search_choices("image", "7318", None, 0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(choices[0].id, 7318);
        // This path.data entry names a TGA that is not a standalone package entry.
        let (_, missing) = resources.search_choices("image", "31344", None, 0, 10).unwrap();
        assert_eq!(missing, 0);
    }

    #[test]
    fn reads_multiline_skill_descriptions() {
        let Some(dir) = client() else { return };
        let res = Resources::new(inspect(&dir).unwrap());
        let raw = res.text(table::SKILLS, 2181).expect("Heavy Blow description");
        assert!(raw.lines().count() > 1, "physical lines were lost: {raw:?}");
        let description = res.skill_description(218).expect("skill 218 description");
        assert!(description.lines().count() > 1, "popover description was truncated: {description:?}");
        assert!(description.contains("^ffffff") && description.contains("^ffcb4a"), "skill colours were lost: {description:?}");
    }

    #[test]
    fn searches_client_resources_for_picker() {
        let Some(dir) = client() else { return };
        let res = Resources::new(inspect(&dir).unwrap());
        let (choices, total) = res.search_choices("skill", "Heavy Blow", None, 0, 10).unwrap();
        assert!(total > 0);
        let heavy = choices.iter().find(|choice| choice.id == 218).expect("skill 218 in picker results");
        assert!(heavy.name.contains("Heavy Blow"));
        assert!(heavy.description.as_deref().is_some_and(|text| text.lines().count() > 1));

        let (first, all) = res.search_choices("skill", "", None, 0, 10).unwrap();
        let (second, again) = res.search_choices("skill", "", None, 10, 10).unwrap();
        assert_eq!(all, again);
        assert_eq!(first.len(), 10);
        assert_eq!(second.len(), 10);
        assert!(first.iter().all(|a| second.iter().all(|b| a.id != b.id)));
    }

    #[test]
    fn reads_title_definitions_from_interfaces() {
        let Some(dir) = client() else { return };
        let res = Resources::new(inspect(&dir).unwrap());
        assert!(res.titles().unwrap().len() > 1000);
        assert_eq!(res.title_name(1001).as_deref(), Some("The Pinnacle"));
        let description = res.title_description(1001).expect("title 1001 description");
        assert!(description.starts_with("^ffbc3cThe Pinnacle\n") && description.lines().count() > 2, "bad title tooltip: {description:?}");
    }
}
