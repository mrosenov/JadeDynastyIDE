pub mod align;
pub mod analyze;
pub mod compare;
pub mod coverage;
pub mod decode;
pub mod edit;
pub mod export;
pub mod format;
pub mod import;
pub mod picker;
pub mod problems;
pub mod reader;
pub mod refs;
pub mod save;
pub mod search;
pub mod talk;
pub mod translation;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use serde::Serialize;

use align::{align, Match};
use decode::{decode_record, gap_node, guess_name, read_wstr, Annotation, Node};
use format::{builtin_layout, Catalog, Field, Layout, LayoutMeta, ListDef, ListHead, Marker, MarkerKind, Ty};
use reader::{ElementsFile, Segment, SegmentKind};

use crate::client::Resources;

/// Where a list's definition comes from and how well it fits the records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutFit {
    /// The file's own layout, same record size.
    Exact,
    /// The file's own layout, records are larger (tail shown as unknown).
    Partial,
    /// Another version's definition with the same record size.
    Borrowed,
    /// Another version's definition of a smaller, older struct.
    Grown,
    /// Only the list's name is known.
    Named,
    None,
}

/// How the list boundaries were found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParseMode {
    /// With the marker table of a layout for this version.
    Layout,
    /// With the marker table of another version's layout.
    Markers,
    /// By recognising segments from their content.
    Detected,
}

struct Resolved {
    name: Option<String>,
    struct_name: Option<String>,
    key: Option<String>,
    /// Definition used for fields: (layout index, list slot in that layout).
    def: Option<(usize, usize)>,
    fit: LayoutFit,
}

pub struct Document {
    pub path: String,
    pub file: ElementsFile,
    catalog: Arc<Catalog>,
    mode: ParseMode,
    primary: Option<usize>,
    markers_from: Option<usize>,
    lists: Vec<Resolved>,
    by_struct: HashMap<String, Vec<usize>>,
    ids: Vec<OnceLock<HashMap<u32, usize>>>,
    /// Edits made since the file was opened (in memory until saved).
    pub(crate) edits: edit::Journal,
    /// Integer fields that may hold other records' IDs (for "referenced by").
    sites: OnceLock<Vec<refs::Site>>,
    /// Every record's ID and lowercased name, for Find.
    find_index: OnceLock<Vec<FindEntry>>,
    /// The NPC dialogs, parsed on first use.
    talks: OnceLock<Result<TalkData, String>>,
    /// The game client's paths and icons, when a client folder is set.
    pub resources: Option<Arc<Resources>>,
    /// Size and time of the file on disk when it was read or saved.
    disk: Option<save::DiskStamp>,
    /// Files a backup was made of (or that were new) in this session.
    backed_up: HashSet<std::path::PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSummary {
    pub index: usize,
    pub name: String,
    pub key: Option<String>,
    pub struct_name: Option<String>,
    pub item_size: usize,
    pub count: usize,
    pub offset: usize,
    pub layout: LayoutFit,
    /// Layout the field definition comes from.
    pub layout_id: Option<String>,
    pub layout_size: Option<usize>,
    /// The definition comes from a user layout written in the schema editor.
    pub custom: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub path: String,
    pub file_size: usize,
    pub version: u32,
    pub raw_version: u32,
    pub timestamp: u32,
    pub exporter: Option<String>,
    pub parse_mode: ParseMode,
    pub layout_id: Option<String>,
    pub layout_source: Option<String>,
    /// The layout's list count was never checked against a real file.
    pub layout_unverified: bool,
    pub layout_custom: bool,
    pub markers_from: Option<String>,
    pub talk_count: u32,
    pub lists: Vec<ListSummary>,
    pub segments: Vec<Segment>,
}

#[derive(Serialize)]
pub struct RecordRow {
    pub index: usize,
    pub id: u32,
    pub name: String,
    /// Path ID of the record's item icon, if the client has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    /// The item's name colour in the game (item_color.txt), when not white.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// The client's text for a record (configs.pck), and the table it comes from.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameText {
    pub text: String,
    pub source: &'static str,
}

/// A record found by Find.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindHit {
    pub list: usize,
    pub index: usize,
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    /// "id" (the record's ID is the query) or "name" (its name contains it).
    pub how: &'static str,
}

struct TalkData {
    talks: Vec<talk::Talk>,
    by_id: HashMap<u32, usize>,
    /// Records whose `id_dialog` opens each dialog, by dialog ID.
    users: HashMap<u32, Vec<TalkUser>>,
}

/// A record that opens a dialog (through its `id_dialog` field).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkUser {
    pub list: usize,
    pub row: usize,
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkSummary {
    pub index: usize,
    pub id: u32,
    pub title: String,
    pub windows: usize,
    pub options: usize,
    /// Records that open the dialog.
    pub users: usize,
    /// Name of the first record that opens it, e.g. the service's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_by: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkDetail {
    pub index: usize,
    #[serde(flatten)]
    pub talk: talk::Talk,
    pub users: Vec<TalkUser>,
}

struct FindEntry {
    list: usize,
    index: usize,
    id: u32,
    name: String,
    lower: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindResult {
    pub hits: Vec<FindHit>,
    /// Matches in all, including those past the limit.
    pub total: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordDetail {
    pub list: usize,
    pub index: usize,
    pub file_offset: usize,
    pub layout: LayoutFit,
    pub layout_id: Option<String>,
    pub layout_size: Option<usize>,
    pub bytes: Vec<u8>,
    pub nodes: Vec<Node>,
    /// Path ID of the record's item icon, if the client has it.
    pub icon: Option<u32>,
    /// The record as the file was opened, when edits changed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<Vec<u8>>,
    /// Created by an edit (a clone).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub added: bool,
    /// The client's description of the record (configs.pck).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_text: Option<GameText>,
    /// The name colour in the game, when not white.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_color: Option<String>,
}

/// A list that fields can refer to by its struct.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefTarget {
    pub list: usize,
    pub name: String,
    pub struct_name: String,
}

/// What the schema editor needs to edit one list.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSchema {
    pub list: usize,
    pub item_size: usize,
    pub count: usize,
    /// Layout an edit is saved into: the file's own, or a new one.
    pub target_layout: String,
    pub target_exists: bool,
    /// The definition in use now (the file's own or borrowed), if any.
    pub def: Option<ListDef>,
    pub def_layout: Option<String>,
    pub fit: LayoutFit,
    /// The target layout's definition differs from the built-in one.
    pub custom: bool,
    /// The built-in layout with the target id has a definition for this list.
    pub has_builtin: bool,
}

/// How well a layout describes a file parsed with its marker table.
fn score(layout: &Layout, file: &ElementsFile) -> i64 {
    let mut score = 0;
    for (i, block) in file.lists.iter().enumerate() {
        match layout.head(i).and_then(|h| h.size) {
            Some(size) if size == block.item_size => score += 2,
            Some(size) if size < block.item_size => score += 1,
            Some(_) => score -= 3,
            None => {}
        }
    }
    if layout.lists.len() == file.lists.len() {
        score += 1;
    }
    score
}

/// The marker table a parsed file actually has.
fn markers_of(file: &ElementsFile) -> Vec<Marker> {
    file.segments
        .iter()
        .filter_map(|s| {
            let kind = match s.kind {
                SegmentKind::Checksum => MarkerKind::Checksum,
                SegmentKind::Exporter => MarkerKind::Exporter,
                SegmentKind::Tag => MarkerKind::Tag,
                _ => return None,
            };
            Some(Marker { before: s.before?, kind })
        })
        .collect()
}

/// A layout's definition, usable for fields at this record size.
fn usable(head: &ListHead, size: usize) -> bool {
    head.has_fields() && head.size.is_some_and(|s| s <= size)
}

impl Document {
    pub fn open(path: String, catalog: Arc<Catalog>) -> Result<Self, String> {
        let disk = save::DiskStamp::of(std::path::Path::new(&path));
        let data = std::fs::read(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
        let mut doc = Self::from_bytes(path, data, catalog)?;
        doc.disk = disk;
        Ok(doc)
    }

    pub fn from_bytes(path: String, data: Vec<u8>, catalog: Arc<Catalog>) -> Result<Self, String> {
        if data.len() < 8 {
            return Err("File is too small to be elements.data".into());
        }
        let version = u32::from_le_bytes(data[0..4].try_into().unwrap()) & 0xffff;
        let index_of = |layout: &Layout| catalog.layouts.iter().position(|l| l.id == layout.id).unwrap();

        // 1. A layout made for this version, the best-fitting one if several.
        let mut best: Option<(i64, usize, ElementsFile)> = None;
        for (i, layout) in catalog.layouts.iter().enumerate().filter(|(_, l)| l.version == version) {
            if let Ok(file) = ElementsFile::parse_with(data.clone(), &layout.markers) {
                let s = score(layout, &file);
                if best.as_ref().is_none_or(|(b, ..)| s > *b) {
                    best = Some((s, i, file));
                }
            }
        }
        let (file, mode, primary, markers_from) = if let Some((_, i, file)) = best {
            (file, ParseMode::Layout, Some(i), Some(i))
        } else if let Some((i, file)) = catalog
            .marker_tables(version)
            .into_iter()
            .filter(|l| l.version != version)
            .find_map(|l| ElementsFile::parse_with(data.clone(), &l.markers).ok().map(|f| (index_of(l), f)))
        {
            // 2. Another version's marker table that fits exactly.
            (file, ParseMode::Markers, None, Some(i))
        } else {
            // 3. Recognise segments by content.
            let file = ElementsFile::parse_detect(data).map_err(|e| format!("Not a readable elements.data: {e}"))?;
            (file, ParseMode::Detected, None, None)
        };

        let lists = Self::resolve(&catalog, &file, primary);
        let mut by_struct: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in lists.iter().enumerate() {
            if let Some(s) = &r.struct_name {
                by_struct.entry(s.clone()).or_default().push(i);
            }
        }
        let ids = (0..file.lists.len()).map(|_| OnceLock::new()).collect();
        let file_counts: Vec<usize> = file.lists.iter().map(|l| l.count).collect();
        Ok(Self { path, file, catalog, mode, primary, markers_from, lists, by_struct, ids, sites: OnceLock::new(), find_index: OnceLock::new(), talks: OnceLock::new(), edits: edit::Journal::new(file_counts), resources: None, disk: None, backed_up: HashSet::new() })
    }

    /// Re-reads the same bytes with a new catalog (after a schema edit).
    pub fn reload(&self, catalog: Arc<Catalog>) -> Result<Self, String> {
        let mut doc = Self::from_bytes(self.path.clone(), self.file.data.clone(), catalog)?;
        doc.resources = self.resources.clone();
        // The data already holds the edits; keep their history.
        doc.edits = self.edits.clone();
        doc.disk = self.disk.clone();
        doc.backed_up = self.backed_up.clone();
        Ok(doc)
    }

    /// Offset of the field standing for the record's icon (display "icon").
    fn icon_field(def: Option<&ListDef>) -> Option<usize> {
        def?.fields
            .iter()
            .find(|f| f.display.as_deref() == Some("icon") && matches!(f.t, Ty::I32 | Ty::U32))
            .map(|f| f.off)
    }

    /// The record's icon path ID, if the client has an icon for it.
    fn record_icon(&self, bytes: &[u8], icon_at: Option<usize>) -> Option<u32> {
        let res = self.resources.as_ref()?;
        let off = icon_at?;
        let id = u32::from_le_bytes(bytes.get(off..off + 4)?.try_into().ok()?);
        (id > 0 && res.item_icon(id).is_some()).then_some(id)
    }

    /// Picks, per list: the file's own layout, else a definition borrowed
    /// from another layout by size alignment, else whatever name is known.
    fn resolve(catalog: &Catalog, file: &ElementsFile, primary: Option<usize>) -> Vec<Resolved> {
        let sizes = file.item_sizes();
        let markers = markers_of(file);
        let version = file.version();

        let mut donors: Vec<usize> = (0..catalog.layouts.len()).filter(|&i| Some(i) != primary).collect();
        donors.sort_by_key(|&i| (catalog.layouts[i].version.abs_diff(version), catalog.layouts[i].id.clone()));
        let alignments: Vec<Vec<Option<Match>>> =
            donors.iter().map(|&d| align(&sizes, &markers, &catalog.layouts[d])).collect();

        let donor_def = |i: usize, exact: bool| {
            donors.iter().zip(&alignments).find_map(|(&layout, matches)| {
                let m = matches[i]?;
                if matches!(m, Match::Exact(_)) != exact {
                    return None;
                }
                let head = catalog.layouts[layout].head(m.donor())?;
                (usable(head, sizes[i]) && !head.is_placeholder()).then_some((layout, m.donor()))
            })
        };
        // Only list heads are needed here; fields are parsed when a list is used.
        let def_of = |(l, i): (usize, usize)| catalog.layouts[l].head(i).unwrap();

        (0..sizes.len())
            .map(|i| {
                let size = sizes[i];
                let own = primary.filter(|&l| catalog.layouts[l].head(i).is_some()).map(|l| (l, i));
                let own_real = own.filter(|&o| !def_of(o).is_placeholder());
                let fit_of = |o: (usize, usize)| {
                    if def_of(o).size == Some(size) {
                        LayoutFit::Exact
                    } else {
                        LayoutFit::Partial
                    }
                };

                let (def, fit) = if let Some(o) = own_real.filter(|&o| usable(def_of(o), size)) {
                    (Some(o), fit_of(o))
                } else if let Some(found) = donor_def(i, true) {
                    (Some(found), LayoutFit::Borrowed)
                } else if let Some(found) = donor_def(i, false) {
                    (Some(found), LayoutFit::Grown)
                } else if let Some(o) = own.filter(|&o| usable(def_of(o), size)) {
                    (Some(o), fit_of(o))
                } else {
                    (None, if own.is_some() { LayoutFit::Named } else { LayoutFit::None })
                };

                // The file's own (non-placeholder) name wins over a borrowed one.
                let named = own_real.or(def).or(own).map(def_of);
                Resolved {
                    name: named.map(|d| d.name.clone()),
                    struct_name: named.and_then(|d| d.struct_name.clone()),
                    key: named.and_then(|d| d.key.clone()),
                    def,
                    fit,
                }
            })
            .collect()
    }

    fn layout(&self, index: usize) -> &Layout {
        &self.catalog.layouts[index]
    }

    fn def(&self, list: usize) -> Option<(&Layout, &ListDef)> {
        let (l, i) = self.lists.get(list)?.def?;
        let layout = self.layout(l);
        Some((layout, layout.list(i)?))
    }

    fn is_custom(&self, layout: &Layout, list: usize) -> bool {
        self.catalog.user.contains(&(layout.id.clone(), list))
    }

    fn list_name(&self, list: usize) -> String {
        self.lists[list].name.clone().unwrap_or_else(|| format!("List {list}"))
    }

    /// Offset and size of the record's name string, if the definition has one.
    fn name_field(def: Option<&ListDef>) -> Option<(usize, usize)> {
        let def = def?;
        let wstr = |f: &&Field| matches!(f.t, Ty::Wstr { .. });
        let field = def
            .fields
            .iter()
            .filter(wstr)
            .find(|f| f.name.eq_ignore_ascii_case("name"))
            .or_else(|| def.fields.iter().find(wstr))?;
        Some((field.off, field.t.size()))
    }

    fn record_name(bytes: &[u8], name_at: Option<(usize, usize)>) -> String {
        match name_at {
            Some((off, size)) if off + size <= bytes.len() => read_wstr(&bytes[off..off + size]),
            _ => guess_name(bytes).unwrap_or_default(),
        }
        .chars()
        .take(64)
        .collect()
    }

    fn record_id(bytes: &[u8]) -> u32 {
        bytes.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0)
    }

    /// Character classes from CHARACTER_CLASS_CONFIG: (character_class_id, name), in record order.
    /// Task class requirements store these IDs.
    pub fn character_classes(&self) -> Vec<(u32, String)> {
        let Some(list) = (0..self.lists.len()).find(|&list| self.lists[list].struct_name.as_deref() == Some("CHARACTER_CLASS_CONFIG")) else { return Vec::new() };
        let Some((_, def)) = self.def(list) else { return Vec::new() };
        let Some(class) = def.fields.iter().find(|field| field.name == "character_class_id" && field.t.size() == 4) else { return Vec::new() };
        let name_at = Self::name_field(Some(def));
        (0..self.file.lists[list].count)
            .filter_map(|row| {
                let bytes = self.file.record(list, row)?;
                let id = u32::from_le_bytes(bytes.get(class.off..class.off + 4)?.try_into().ok()?);
                Some((id, Self::record_name(bytes, name_at)))
            })
            .collect()
    }

    /// ID → row for a list. Later records win, as in the client.
    fn id_index(&self, list: usize) -> &HashMap<u32, usize> {
        self.ids[list].get_or_init(|| {
            (0..self.file.lists[list].count)
                .map(|row| (Self::record_id(self.file.record(list, row).unwrap()), row))
                .collect()
        })
    }

    pub fn summary(&self) -> FileSummary {
        let lists = self
            .file
            .lists
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let r = &self.lists[index];
                let def = self.def(index);
                ListSummary {
                    index,
                    name: self.list_name(index),
                    key: r.key.clone(),
                    struct_name: r.struct_name.clone(),
                    item_size: block.item_size,
                    count: block.count,
                    offset: block.header_offset,
                    layout: r.fit,
                    layout_id: def.map(|(l, _)| l.id.clone()),
                    layout_size: def.and_then(|(_, d)| d.size),
                    custom: def.is_some_and(|(l, _)| self.is_custom(l, r.def.unwrap().1)),
                }
            })
            .collect();
        let primary = self.primary.map(|i| self.layout(i));
        FileSummary {
            path: self.path.clone(),
            file_size: self.file.data.len(),
            version: self.file.version(),
            raw_version: self.file.raw_version,
            timestamp: self.file.timestamp,
            exporter: self.file.exporter.clone(),
            parse_mode: self.mode,
            layout_id: primary.map(|l| l.id.clone()),
            layout_source: primary.map(|l| l.source.clone()),
            layout_unverified: primary.is_some_and(|l| l.list_count_unverified),
            layout_custom: primary
                .is_some_and(|l| self.catalog.user_layouts.contains(&l.id) || self.catalog.user.iter().any(|(id, _)| *id == l.id)),
            markers_from: self.markers_from.map(|i| self.layout(i).id.clone()),
            talk_count: self.file.talk_count,
            lists,
            segments: self.file.segments.clone(),
        }
    }

    pub fn records(&self, list: usize) -> Result<Vec<RecordRow>, String> {
        let block = self.file.lists.get(list).ok_or("No such list")?;
        let def = self.def(list).map(|(_, d)| d);
        let name_at = Self::name_field(def);
        let icon_at = Self::icon_field(def);
        Ok((0..block.count)
            .map(|index| {
                let bytes = self.file.record(list, index).unwrap();
                RecordRow {
                    index,
                    id: Self::record_id(bytes),
                    name: Self::record_name(bytes, name_at),
                    icon: self.record_icon(bytes, icon_at),
                    color: self.name_color(list, Self::record_id(bytes)),
                }
            })
            .collect())
    }

    /// Enum labels and cross-list references for a field value.
    fn annotate(&self, layout: Option<&Layout>, field: &Field, value: i64) -> Annotation {
        let mut a = Annotation::default();
        if let Some(set) = field.e.as_deref().and_then(|key| self.catalog.enum_set(layout, key)) {
            a.hint = set.label_for(value);
            a.set = field.e.clone();
            return a;
        }
        // Skill, buff and title IDs: their names and descriptions from the client.
        if let (Some(role @ ("skill" | "buff" | "title")), Some(res)) = (field.display.as_deref(), &self.resources) {
            if value > 0 && value <= u32::MAX as i64 {
                let id = value as u32;
                let (name, description) = match role {
                    "skill" => (res.skill_name(id), res.skill_description(id)),
                    "buff" => (res.buff_name(id), res.buff_description(id)),
                    "title" => (res.title_name(id), res.title_description(id)),
                    _ => unreachable!(),
                };
                a.hint = Some(name.unwrap_or_else(|| format!("no such {role}")));
                a.description = description;
                return a;
            }
        }
        // Path, atlas icon and standalone image fields hold path.data IDs.
        if let (Some(display @ ("path" | "icon" | "image")), Some(res)) = (field.display.as_deref(), &self.resources) {
            if value > 0 && value <= u32::MAX as i64 {
                let id = value as u32;
                if let Some(path) = res.path(id) {
                    a.hint = Some(path.to_string());
                    if display == "icon" && res.item_icon(id).is_some() {
                        a.icon = Some(id);
                    } else if display == "image" && res.has_image(id) {
                        a.image = Some(id);
                    }
                } else if res.paths().is_ok() {
                    a.hint = Some("not in path.data".into());
                }
                return a;
            }
        }
        // Services' `id_dialog` opens an NPC dialog.
        if field.name.eq_ignore_ascii_case("id_dialog") && value > 0 && value <= u32::MAX as i64 {
            if let Ok(data) = self.talk_data() {
                match data.by_id.get(&(value as u32)) {
                    Some(&index) => {
                        let title = data.talks[index].title();
                        a.hint = Some(format!("Dialog › {}", if title.is_empty() { "untitled" } else { &title }));
                        a.talk = Some(index);
                    }
                    None => a.hint = Some("no such dialog".into()),
                }
                return a;
            }
        }
        if field.refs.is_empty() || value <= 0 || value > u32::MAX as i64 {
            return a;
        }
        let targets: Vec<usize> = field.refs.iter().filter_map(|s| self.by_struct.get(s)).flatten().copied().collect();
        for &list in &targets {
            if let Some(&row) = self.id_index(list).get(&(value as u32)) {
                let bytes = self.file.record(list, row).unwrap();
                let name = Self::record_name(bytes, Self::name_field(self.def(list).map(|(_, d)| d)));
                a.hint = Some(format!("{} › {}", self.list_name(list), if name.is_empty() { "unnamed" } else { &name }));
                a.link = Some((list, row));
                return a;
            }
        }
        if !targets.is_empty() {
            a.hint = Some(format!("not found in {}", self.list_name(targets[0])));
        }
        a
    }

    fn detail(&self, list: usize, index: usize, def: Option<(Option<&Layout>, &ListDef)>, fit: LayoutFit) -> Result<RecordDetail, String> {
        let bytes = self.file.record(list, index).ok_or("No such record")?;
        let nodes = match def {
            Some((layout, def)) => {
                let annotate = |f: &Field, v: i64| self.annotate(layout, f, v);
                decode_record(bytes, &def.fields, &annotate)
            }
            None => vec![gap_node(bytes, 0, bytes.len())],
        };
        Ok(RecordDetail {
            list,
            index,
            file_offset: self.file.record_offset(list, index).unwrap(),
            layout: fit,
            layout_id: def.and_then(|(l, _)| l.map(|l| l.id.clone())),
            layout_size: def.and_then(|(_, d)| d.size),
            icon: self.record_icon(bytes, Self::icon_field(def.map(|(_, d)| d))),
            bytes: bytes.to_vec(),
            nodes,
            original: self.original(list, index).map(<[u8]>::to_vec),
            added: self.is_added(list, index),
            game_text: self.game_text(list, Self::record_id(bytes)),
            name_color: self.name_color(list, Self::record_id(bytes)),
        })
    }

    /// The ID space the client registers a list's records in.
    fn space_of(&self, list: usize) -> Option<refs::IdSpace> {
        self.lists[list].struct_name.as_deref().filter(|s| !s.to_ascii_uppercase().starts_with("UNKNOWN")).map(refs::registry_space)
    }

    /// The client's text for a record: an item's or monster's description,
    /// or an addon's text.
    pub fn game_text(&self, list: usize, id: u32) -> Option<GameText> {
        use crate::client::table;
        let res = self.resources.as_ref()?;
        if id == 0 {
            return None;
        }
        let sources: &[&'static str] = match self.space_of(list)? {
            refs::IdSpace::Essence => &[table::ITEM_DESC, table::MONSTERS],
            refs::IdSpace::Addon => &[table::ADDONS],
            _ => &[],
        };
        sources.iter().find_map(|&source| Some(GameText { text: res.text(source, id)?, source }))
    }

    /// An item's name colour (item_color.txt lists items by their ID).
    pub fn name_color(&self, list: usize, id: u32) -> Option<String> {
        if self.space_of(list)? != refs::IdSpace::Essence {
            return None;
        }
        self.resources.as_ref()?.item_color(id).map(str::to_string)
    }

    pub fn record(&self, list: usize, index: usize) -> Result<RecordDetail, String> {
        let r = self.lists.get(list).ok_or("No such list")?;
        let def = self.def(list).map(|(l, d)| (Some(l), d));
        self.detail(list, index, def, r.fit)
    }

    /// Resolves an ID exactly as the elements loader's shared Essence map:
    /// lists are visited in load order and a later record replaces an earlier one.
    pub fn resolve_essence_id(&self, id: u32) -> Option<(usize, usize, String)> {
        for list in (0..self.lists.len()).rev() {
            if self.space_of(list) != Some(refs::IdSpace::Essence) {
                continue;
            }
            let Some(&row) = self.id_index(list).get(&id) else { continue };
            let bytes = self.file.record(list, row)?;
            let name = Self::record_name(bytes, Self::name_field(self.def(list).map(|(_, definition)| definition)));
            let record = if name.is_empty() { "unnamed" } else { &name };
            return Some((list, row, format!("{} › {record}", self.list_name(list))));
        }
        None
    }

    /// The name of the record an Essence ID resolves to (none when it has no name).
    pub fn essence_name(&self, id: u32) -> Option<String> {
        let (list, row, _) = self.resolve_essence_id(id)?;
        let name = Self::record_name(self.file.record(list, row)?, Self::name_field(self.def(list).map(|(_, definition)| definition)));
        (!name.trim().is_empty()).then_some(name)
    }

    /// The structure name (e.g. `MONSTER_ESSENCE`, upper case) and label of the record an Essence ID resolves to.
    pub fn essence_struct(&self, id: u32) -> Option<(String, String)> {
        let (list, _, label) = self.resolve_essence_id(id)?;
        Some((self.lists[list].struct_name.as_deref().unwrap_or("").to_ascii_uppercase(), label))
    }

    // ------------------------------------------------------------ schema editing

    /// Id of the layout schema edits for this file are saved into: the file's
    /// own layout, or a new one for its version.
    pub fn edit_target(&self) -> String {
        if let Some(i) = self.primary {
            return self.layout(i).id.clone();
        }
        let base = format!("v{}", self.file.version());
        if self.catalog.find(&base).is_none() {
            return base;
        }
        // A layout with that id exists but does not fit this file.
        format!("{base}-{}lists", self.file.lists.len())
    }

    pub fn list_schema(&self, list: usize) -> Result<ListSchema, String> {
        let block = self.file.lists.get(list).ok_or("No such list")?;
        let r = &self.lists[list];
        let target = self.edit_target();
        let target_layout = self.catalog.find(&target);
        let def = self.def(list);
        Ok(ListSchema {
            list,
            item_size: block.item_size,
            count: block.count,
            target_exists: target_layout.is_some(),
            custom: target_layout.is_some_and(|l| self.is_custom(l, list)),
            has_builtin: builtin_layout(&target).is_some_and(|b| b.head(list).is_some()),
            target_layout: target,
            def: def.map(|(_, d)| d.clone()).or_else(|| {
                // Name-only lists: start from the known name.
                r.name.clone().map(|name| ListDef {
                    key: r.key.clone(),
                    name,
                    struct_name: r.struct_name.clone(),
                    size: None,
                    fields: Vec::new(),
                })
            }),
            def_layout: def.map(|(l, _)| l.id.clone()),
            fit: r.fit,
        })
    }

    /// The layout.json to write when the edit target does not exist yet: a
    /// new layout for this file's version, with the file's own marker table.
    pub fn new_target_meta(&self) -> Option<LayoutMeta> {
        let target = self.edit_target();
        self.catalog.find(&target).is_none().then(|| LayoutMeta {
            id: target,
            version: self.file.version(),
            source: format!("Written in the schema editor (from {})", self.path),
            markers: markers_of(&self.file),
            list_count: self.file.lists.len(),
            enums: HashMap::new(),
            list_count_unverified: false,
        })
    }

    /// Decodes a record with a draft definition, without saving it.
    pub fn preview(&self, list: usize, index: usize, def: &ListDef) -> Result<RecordDetail, String> {
        let block = self.file.lists.get(list).ok_or("No such list")?;
        let target = self.catalog.find(&self.edit_target()).map(|l| &**l);
        let fit = match def.size {
            _ if def.fields.is_empty() => LayoutFit::None,
            Some(s) if s == block.item_size => LayoutFit::Exact,
            Some(s) if s < block.item_size => LayoutFit::Partial,
            _ => LayoutFit::None,
        };
        self.detail(list, index, Some((target, def)), fit)
    }

    /// This file's lists that have a struct name: possible ref targets.
    pub fn ref_targets(&self) -> Vec<RefTarget> {
        let mut out: Vec<RefTarget> = self
            .lists
            .iter()
            .enumerate()
            .filter_map(|(list, r)| {
                Some(RefTarget { list, name: self.list_name(list), struct_name: r.struct_name.clone()? })
            })
            .collect();
        out.sort_by(|a, b| a.struct_name.cmp(&b.struct_name).then(a.list.cmp(&b.list)));
        out
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    // ------------------------------------------------------------ referenced by

    fn sites(&self) -> &[refs::Site] {
        self.sites.get_or_init(|| {
            let mut out = Vec::new();
            for list in 0..self.lists.len() {
                if let Some((_, def)) = self.def(list) {
                    refs::sites_of(list, &def.fields, &mut out);
                }
            }
            out
        })
    }

    /// Records whose fields point at this record's ID: fields whose refs name
    /// its struct, and ID-named fields of the same ID space.
    fn talk_data(&self) -> Result<&TalkData, String> {
        self.talks
            .get_or_init(|| {
                let segment = self.file.segments.iter().find(|s| s.kind == SegmentKind::Talk).ok_or("This file has no NPC dialog block")?;
                let talks = talk::parse(&self.file.data, segment.offset)?;
                let by_id = talks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
                // Lists with an `id_dialog` field open dialogs.
                let mut users: HashMap<u32, Vec<TalkUser>> = HashMap::new();
                for list in 0..self.file.lists.len() {
                    let Some((_, def)) = self.def(list) else { continue };
                    let Some(field) = def.fields.iter().find(|f| f.name.eq_ignore_ascii_case("id_dialog") && f.t.size() == 4) else { continue };
                    let name_at = Self::name_field(Some(def));
                    for row in 0..self.file.lists[list].count {
                        let bytes = self.file.record(list, row).unwrap();
                        let Some(v) = bytes.get(field.off..field.off + 4) else { continue };
                        let talk_id = u32::from_le_bytes(v.try_into().unwrap());
                        if talk_id != 0 {
                            users.entry(talk_id).or_default().push(TalkUser { list, row, id: Self::record_id(bytes), name: Self::record_name(bytes, name_at) });
                        }
                    }
                }
                Ok(TalkData { talks, by_id, users })
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The advanced search (see [`search`]).
    pub fn search(&self, query: &search::Query) -> Result<search::Report, String> {
        self.search_limited(query, search::LIMIT)
    }

    pub fn search_limited(&self, query: &search::Query, limit: usize) -> Result<search::Report, String> {
        let views: Vec<search::ListView> = self
            .file
            .lists
            .iter()
            .enumerate()
            .map(|(index, block)| search::ListView {
                index,
                item_size: block.item_size,
                count: block.count,
                data: &self.file.data,
                data_offset: block.data_offset,
                slots: self.def(index).map(|(_, d)| search::slots(d, block.item_size)).unwrap_or_default(),
            })
            .collect();
        let set_of = |key: &str| self.catalog.enum_set(None, key);
        let describe = |list: usize, bytes: &[u8]| {
            let def = self.def(list).map(|(_, d)| d);
            (Self::record_id(bytes), Self::record_name(bytes, Self::name_field(def)), self.record_icon(bytes, Self::icon_field(def)))
        };
        search::Searcher { set_of: &set_of, describe: &describe, limit }.run(query, &views)
    }

    /// Field names of the file's lists, for search suggestions.
    pub fn field_names(&self, list: Option<usize>) -> Vec<search::FieldName> {
        match list {
            Some(i) => search::field_names(self.file.lists.get(i).and_then(|list| Some((self.def(i)?.1, list.item_size))).into_iter()),
            None => search::field_names((0..self.file.lists.len()).filter_map(|i| Some((self.def(i)?.1, self.file.lists[i].item_size)))),
        }
    }

    /// Every NPC dialog, in file order.
    pub fn talks(&self) -> Result<Vec<TalkSummary>, String> {
        let data = self.talk_data()?;
        Ok(data
            .talks
            .iter()
            .enumerate()
            .map(|(index, t)| {
                let users = data.users.get(&t.id);
                TalkSummary {
                    index,
                    id: t.id,
                    title: t.title(),
                    windows: t.windows.len(),
                    options: t.windows.iter().map(|w| w.options.len()).sum(),
                    users: users.map_or(0, Vec::len),
                    used_by: users.and_then(|u| u.first()).map(|u| u.name.clone()),
                }
            })
            .collect())
    }

    pub fn talk(&self, index: usize) -> Result<TalkDetail, String> {
        let data = self.talk_data()?;
        let talk = data.talks.get(index).ok_or("No such dialog")?.clone();
        let users = data.users.get(&talk.id).cloned().unwrap_or_default();
        Ok(TalkDetail { index, talk, users })
    }

    /// Records of every list whose ID is the query (when it is a number) or
    /// whose name contains it, case-insensitively. ID matches come first,
    /// then exact names, names starting with the query, and the rest, each
    /// in file order.
    pub fn find(&self, query: &str, limit: usize) -> FindResult {
        let query = query.trim();
        let id = query.parse::<u32>().ok().filter(|&n| n != 0);
        let needle = query.to_lowercase();
        if query.is_empty() {
            return FindResult { hits: vec![], total: 0 };
        }
        let index = self.find_entries();
        let mut ranked: Vec<(u8, &FindEntry)> = index
            .iter()
            .filter_map(|e| {
                let rank = if Some(e.id) == id {
                    0
                } else if e.lower.is_empty() || !e.lower.contains(&needle) {
                    return None;
                } else if e.lower == needle {
                    1
                } else if e.lower.starts_with(&needle) {
                    2
                } else {
                    3
                };
                Some((rank, e))
            })
            .collect();
        // Stable: file order within a rank.
        ranked.sort_by_key(|(rank, _)| *rank);
        let total = ranked.len();
        let hits = ranked
            .into_iter()
            .take(limit)
            .map(|(rank, e)| {
                let bytes = self.file.record(e.list, e.index).unwrap_or_default();
                let icon = self.record_icon(bytes, Self::icon_field(self.def(e.list).map(|(_, d)| d)));
                FindHit { list: e.list, index: e.index, id: e.id, name: e.name.clone(), icon, how: if rank == 0 { "id" } else { "name" } }
            })
            .collect();
        FindResult { hits, total }
    }

    fn find_entries(&self) -> &[FindEntry] {
        self.find_index.get_or_init(|| {
            (0..self.file.lists.len())
                .flat_map(|list| {
                    let name_at = Self::name_field(self.def(list).map(|(_, def)| def));
                    (0..self.file.lists[list].count).filter_map(move |index| {
                        let bytes = self.file.record(list, index)?;
                        let name = Self::record_name(bytes, name_at);
                        Some(FindEntry { list, index, id: Self::record_id(bytes), lower: name.to_lowercase(), name })
                    })
                })
                .collect()
        })
    }

    pub fn referenced_by(&self, list: usize, row: usize) -> Result<refs::ReferencedBy, String> {
        const LIMIT: usize = 500;
        let bytes = self.file.record(list, row).ok_or("No such record")?;
        let id = Self::record_id(bytes);
        let mut out = refs::ReferencedBy { id, referrers: Vec::new(), truncated: false };
        if id == 0 {
            return Ok(out);
        }
        let target = self.lists[list].struct_name.as_deref();
        let space = refs::list_space(target.unwrap_or(""));
        let mut names: HashMap<usize, (Option<(usize, usize)>, Option<usize>)> = HashMap::new();

        for site in self.sites() {
            let how = if target.is_some_and(|t| site.refs.iter().any(|r| r == t)) {
                "declared"
            } else if site.refs.is_empty() && site.space == Some(space) {
                "id"
            } else {
                continue;
            };
            let block = &self.file.lists[site.list];
            if site.off + 4 > block.item_size {
                continue;
            }
            for r in 0..block.count {
                if site.list == list && r == row {
                    continue;
                }
                let at = block.data_offset + r * block.item_size + site.off;
                if u32::from_le_bytes(self.file.data[at..at + 4].try_into().unwrap()) != id {
                    continue;
                }
                if out.referrers.len() == LIMIT {
                    out.truncated = true;
                    break;
                }
                let (name_at, icon_at) = *names.entry(site.list).or_insert_with(|| {
                    let def = self.def(site.list).map(|(_, d)| d);
                    (Self::name_field(def), Self::icon_field(def))
                });
                let rec = self.file.record(site.list, r).unwrap();
                out.referrers.push(refs::Referrer {
                    list: site.list,
                    row: r,
                    id: Self::record_id(rec),
                    name: Self::record_name(rec, name_at),
                    field: site.path.clone(),
                    how,
                    icon: self.record_icon(rec, icon_at),
                });
            }
        }
        out.referrers.sort_by(|a, b| (a.list, a.row, &a.field).cmp(&(b.list, b.row, &b.field)));
        Ok(out)
    }

    // ------------------------------------------------------------ import from other versions

    /// The definitions other layouts have for the same list slot, to copy
    /// into the schema editor. Sorted by version.
    pub fn import_candidates(&self, list: usize) -> Result<Vec<ImportCandidate>, String> {
        let item_size = self.file.lists.get(list).ok_or("No such list")?.item_size;
        let target = self.edit_target();
        let mut out: Vec<ImportCandidate> = self
            .catalog
            .layouts
            .iter()
            .filter(|layout| layout.id != target)
            .filter_map(|layout| {
                layout.head(list).filter(|h| h.has_fields())?;
                let def = layout.list(list)?;
                Some(ImportCandidate {
                    layout_id: layout.id.clone(),
                    version: layout.version,
                    name: def.name.clone(),
                    struct_name: def.struct_name.clone(),
                    size: def.size.unwrap_or(0),
                    item_size,
                    def: def.clone(),
                })
            })
            .collect();
        out.sort_by(|a, b| (a.version, &a.layout_id).cmp(&(b.version, &b.layout_id)));
        Ok(out)
    }
}

/// A definition of the same list slot from another layout.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub layout_id: String,
    pub version: u32,
    pub name: String,
    pub struct_name: Option<String>,
    /// Bytes the definition describes.
    pub size: usize,
    /// Record size of the list in the open file.
    pub item_size: usize,
    pub def: ListDef,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin() -> Arc<Catalog> {
        Arc::new(Catalog::load(None))
    }

    /// Real files are not checked in. Paths are relative to JDIDE_SAMPLES
    /// (default `E:/`); missing samples are skipped with a note.
    fn open(rel: &str) -> Option<Document> {
        let root = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/".into());
        let path = format!("{root}/{rel}");
        if !std::path::Path::new(&path).exists() {
            eprintln!("skipping {rel}: sample not found");
            return None;
        }
        Some(Document::open(path, builtin()).unwrap())
    }

    fn tally(doc: &Document) -> HashMap<LayoutFit, usize> {
        let mut t = HashMap::new();
        for r in &doc.lists {
            *t.entry(r.fit).or_default() += 1;
        }
        t
    }

    const SAMPLES: &[(&str, u32, usize, ParseMode)] = &[
        ("Game Dev/JD/1792/gamed/config/c01/elements.data", 66, 89, ParseMode::Layout),
        ("Game Dev/JD/Tools/JadeEditorFOX/tests/elements - Copy.data", 112, 107, ParseMode::Layout),
        ("Game Dev/JD/zxserver/zgame/gs/config/elements.data", 156, 193, ParseMode::Layout),
        ("Game Dev/JD/1559/gamed/config/elements.data", 158, 230, ParseMode::Layout),
        ("Game Dev/JD/Clean/root/gamed/config/elements.data", 160, 246, ParseMode::Layout),
        ("Games/ForsakenJD/element/data/elements.data", 160, 246, ParseMode::Layout),
        ("Game Dev/JD/1792/gamed/config/elements.data", 165, 318, ParseMode::Layout),
        ("Games/Elite Jade Dynasty - HDN/element/data/elements.data", 165, 318, ParseMode::Layout),
    ];

    #[test]
    fn every_sample_parses_with_its_layout() {
        for &(rel, version, lists, mode) in SAMPLES {
            let Some(doc) = open(rel) else { continue };
            let covered: usize = doc.file.segments.iter().map(|s| s.size).sum();
            assert_eq!(covered, doc.file.data.len(), "{rel}: segments must cover the file");
            assert_eq!(doc.file.version(), version, "{rel}");
            assert_eq!(doc.file.lists.len(), lists, "{rel}");
            assert_eq!(doc.mode, mode, "{rel}");
            assert!(doc.file.talk_count > 0, "{rel}");
            eprintln!("{rel}: {:?}", tally(&doc));
        }
    }

    #[test]
    fn v156_prefers_the_matching_variant() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        assert_eq!(doc.layout(doc.primary.unwrap()).id, "v156");
        assert!(doc.lists.iter().all(|r| r.fit == LayoutFit::Exact));
    }

    #[test]
    fn v165_reads_the_fifth_checksum_slot() {
        let Some(doc) = open("Game Dev/JD/1792/gamed/config/elements.data") else { return };
        let r = &doc.lists[296];
        assert_eq!(r.struct_name.as_deref(), Some("MERGED_STAR_SOUL_LIMIT_CONFIG"));
        assert_eq!(r.fit, LayoutFit::Exact);
        let checksums = doc.file.segments.iter().filter(|s| s.kind == SegmentKind::Checksum).count();
        assert_eq!(checksums, 5);
    }

    #[test]
    fn v158_borrows_most_lists_from_neighbours() {
        let Some(doc) = open("Game Dev/JD/1559/gamed/config/elements.data") else { return };
        let t = tally(&doc);
        assert!(t.get(&LayoutFit::Borrowed).copied().unwrap_or(0) > 150, "{t:?}");
    }

    #[test]
    fn enums_and_refs_resolve() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let equipment = doc.by_struct["EQUIPMENT_ESSENCE"][0];
        let rows = doc.records(equipment).unwrap();
        let row = rows.iter().find(|r| r.id == 132).unwrap();
        let detail = doc.record(equipment, row.index).unwrap();
        let find = |name: &str| detail.nodes.iter().find(|n| n.name == name).unwrap();
        let major = find("id_major_type");
        assert!(major.link.is_some(), "{:?}", major.hint);
        assert_eq!(major.link.unwrap().0, doc.by_struct["EQUIPMENT_MAJOR_TYPE"][0]);
        assert!(find("proc_type").hint.is_some());
    }

    #[test]
    fn picker_limits_a_reference_to_its_declared_list() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let equipment = doc.by_struct["EQUIPMENT_ESSENCE"][0];
        let row = doc.records(equipment).unwrap().into_iter().find(|record| record.id == 132).unwrap().index;
        let node = doc.record(equipment, row).unwrap().nodes.into_iter().find(|node| node.name == "id_major_type").unwrap();
        assert_eq!(node.picker.as_deref(), Some("reference"));
        let request = picker::Request { list: equipment, row, off: node.off, query: node.value.unwrap(), page: 0 };
        let spec = doc.picker_spec(&request).unwrap();
        let page = doc.picker_records(&spec, &request.query, request.page).unwrap();
        assert!(!page.entries.is_empty());
        assert!(page.entries.iter().all(|entry| entry.list == Some(doc.by_struct["EQUIPMENT_MAJOR_TYPE"][0])));
        assert!(page.entries.iter().any(|entry| entry.value.to_string() == request.query));
    }

    #[test]
    fn v112_uses_jade_editor_names_and_enums() {
        let Some(doc) = open("Game Dev/JD/Tools/JadeEditorFOX/tests/elements - Copy.data") else { return };
        assert_eq!(doc.list_name(0), "AddedAttribute");
        let row = doc.records(0).unwrap().into_iter().find(|r| r.id == 1752).unwrap();
        assert_eq!(row.name, "法宝技能_真山河扇");
        let detail = doc.record(0, row.index).unwrap();
        let ty = detail.nodes.iter().find(|n| n.name == "Type").unwrap();
        // The Jade Editor's attribute types are the shared addon types.
        assert!(ty.hint.as_deref().is_some_and(|h| h.starts_with("Bonus Skill")), "{:?}", ty.hint);
    }

    #[test]
    fn dialogs_parse_and_link_to_services() {
        for rel in ["Game Dev/JD/zxserver/zgame/gs/config/elements.data", "Game Dev/JD/1792/gamed/config/elements.data", "Games/ForsakenJD/element/data/elements.data"] {
            let Some(doc) = open(rel) else { continue };
            let talks = doc.talks().unwrap();
            assert_eq!(talks.len(), doc.file.talk_count as usize, "{rel}");
            let seg = doc.file.segments.iter().find(|s| s.kind == SegmentKind::Talk).unwrap();
            let last = doc.talk(talks.len() - 1).unwrap();
            assert_eq!(last.talk.offset + last.talk.size, seg.offset + seg.size, "{rel}: dialogs end at EOF");
            assert!(doc.talk(0).unwrap().talk.windows.iter().any(|w| w.parent == talk::NO_PARENT), "{rel}: a root window");
            let used = talks.iter().filter(|t| t.users > 0).count();
            eprintln!("{rel}: {} dialogs, {used} opened by records", talks.len());
            assert!(used > talks.len() / 4, "{rel}: services open dialogs");
            // A service's id_dialog links to its dialog.
            let service = doc.by_struct["NPC_TALK_SERVICE"][0];
            let row = doc.records(service).unwrap().into_iter().position(|r| r.id != 0).unwrap();
            let node = doc.record(service, row).unwrap().nodes.into_iter().find(|n| n.name == "id_dialog").unwrap();
            let index = node.talk.expect("id_dialog links to a dialog");
            assert_eq!(doc.talk(index).unwrap().talk.id.to_string(), node.value.unwrap());
        }
    }

    #[test]
    fn dialogs_encode_byte_exact_and_text_edits_undo() {
        for rel in ["Game Dev/JD/zxserver/zgame/gs/config/elements.data", "Game Dev/JD/1792/gamed/config/elements.data", "Games/ForsakenJD/element/data/elements.data"] {
            let Some(mut doc) = open(rel) else { continue };
            let parsed = doc.talk_data().unwrap().talks.clone();
            for talk in &parsed {
                assert_eq!(talk::encode_one(talk).unwrap(), doc.file.data[talk.offset..talk.offset + talk.size], "{rel}: dialog {}", talk.id);
            }

            let before = doc.file.data.clone();
            let first = doc.talk(0).unwrap().talk;
            let mut edit = edit::TalkTextEdit {
                text: first.text.clone(),
                windows: first.windows.iter().map(|w| edit::TalkWindowTextEdit {
                    text: w.text.clone(),
                    options: w.options.iter().map(|o| o.text.clone()).collect(),
                }).collect(),
            };
            edit.windows[0].text.push_str(" translated");
            let state = doc.edit_talk_text(0, &edit).unwrap();
            assert_eq!(state.changed_talks, vec![0]);
            let changed = doc.talk(0).unwrap();
            assert!(changed.talk.windows[0].text.ends_with(" translated"));
            assert_eq!(changed.talk.windows[0].id, first.windows[0].id);
            assert_eq!(changed.talk.windows[0].parent, first.windows[0].parent);
            assert_eq!(changed.talk.windows[0].options.iter().map(|o| (o.id, o.param)).collect::<Vec<_>>(), first.windows[0].options.iter().map(|o| (o.id, o.param)).collect::<Vec<_>>());
            doc.undo();
            assert_eq!(doc.file.data, before, "{rel}: undo restores every byte");
            edit.text = "x".repeat(65);
            assert!(doc.edit_talk_text(0, &edit).unwrap_err().contains("64"), "{rel}: fixed title limit");
            assert_eq!(doc.file.data, before, "{rel}: a rejected edit changes nothing");

            // Revert all can mix a record insertion/removal (which moves the
            // trailing dialog block) and a variable-length dialog edit.
            edit.text = first.text.clone();
            edit.windows[0].text.push_str(" again");
            doc.edit_talk_text(0, &edit).unwrap();
            doc.clone_record(3, 0).unwrap();
            doc.revert(None, "Revert all changes");
            assert_eq!(doc.file.data, before, "{rel}: mixed Revert all restores every byte");
            doc.edit_talk_text(0, &edit).unwrap();
            doc.revert_talk(0, "Revert dialog").unwrap();
            assert_eq!(doc.file.data, before, "{rel}: Revert dialog restores every byte");
        }
    }

    #[test]
    fn search_by_conditions_and_values() {
        use search::{Condition, Op, Query, ValueKind};
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let cond = |field: &str, op: Op, value: &str| Condition { field: field.into(), op, value: value.into() };
        // Equipment with a level requirement above 100 that cannot be traded.
        let q = Query::Conditions {
            conditions: vec![cond("require_level", Op::Gt, "100"), cond("proc_type", Op::HasFlags, "16")],
            match_all: true,
            list: None,
        };
        let r = doc.search(&q).unwrap();
        eprintln!("conditions: {} records in {} lists ({} ms)", r.matched_records, r.matched_lists, r.elapsed_ms);
        assert!(r.matched_records > 0);
        for hit in &r.hits {
            let detail = doc.record(hit.list, hit.row).unwrap();
            let level = detail.nodes.iter().find(|n| n.name.eq_ignore_ascii_case("require_level")).unwrap();
            assert!(level.value.as_ref().unwrap().parse::<i64>().unwrap() > 100);
        }
        // A record's ID found by value lands on its id field.
        let row = doc.records(3).unwrap().into_iter().find(|r| r.id > 1000).unwrap();
        let q = Query::Value { value: row.id.to_string(), kind: ValueKind::Int, list: Some(3), include_unknown: false, case_sensitive: false };
        let r = doc.search(&q).unwrap();
        assert!(r.hits.iter().any(|h| h.row == row.index && h.matches.iter().any(|m| m.field == "id")));
        // Text in names, case-insensitive.
        let name: String = row.name.chars().take(3).collect();
        let q = Query::Value { value: name.to_uppercase(), kind: ValueKind::Text, list: Some(3), include_unknown: false, case_sensitive: false };
        assert!(doc.search(&q).unwrap().hits.iter().any(|h| h.row == row.index));
        // Automatic all-field search recognizes both text and numbers, and stays in the selected list.
        let q = Query::Value { value: name.to_uppercase(), kind: ValueKind::Auto, list: Some(3), include_unknown: false, case_sensitive: false };
        let r = doc.search(&q).unwrap();
        assert_eq!(r.scanned_lists, 1);
        assert!(r.hits.iter().all(|h| h.list == 3));
        assert!(r.hits.iter().any(|h| h.row == row.index));
        let q = Query::Value { value: row.id.to_string(), kind: ValueKind::Auto, list: Some(3), include_unknown: false, case_sensitive: false };
        assert!(doc.search(&q).unwrap().hits.iter().any(|h| h.row == row.index && h.matches.iter().any(|m| m.field == "id")));
        // Bad values are reported.
        let q = Query::Conditions { conditions: vec![cond("require_level", Op::Lt, "abc")], match_all: true, list: None };
        assert!(doc.search(&q).is_err());
        assert!(doc.field_names(None).iter().any(|f| f.name == "proc_type" && f.lists > 50));
        assert!(doc.field_names(Some(3)).iter().all(|f| f.lists == 1));
    }

    #[test]
    fn problems_scan_the_whole_file() {
        for rel in ["Game Dev/JD/zxserver/zgame/gs/config/elements.data", "Game Dev/JD/1792/gamed/config/elements.data"] {
            let Some(doc) = open(rel) else { continue };
            let r = doc.problems();
            eprintln!("{rel}: {} ms", r.elapsed_ms);
            for k in &r.kinds {
                eprintln!("  {:?} {:>6}  {}", k.severity, k.count, k.title);
            }
            // Drop tables drop items: their id_obj is no addon reference.
            assert!(!r.problems.iter().any(|p| p.kind == problems::Kind::BrokenRef && p.field.as_deref().is_some_and(|f| f.contains("id_obj"))));
            // Masks are reported per field and bit pattern, not per record.
            assert!(r.kinds.iter().find(|k| k.kind == problems::Kind::UnnamedBits).unwrap().count < 500);
            assert!(r.kinds.iter().any(|k| k.count > 0));
        }
    }

    #[test]
    fn coverage_adds_up() {
        for rel in ["Game Dev/JD/zxserver/zgame/gs/config/elements.data", "Game Dev/JD/1792/gamed/config/elements.data"] {
            let Some(doc) = open(rel) else { continue };
            let rows = doc.coverage();
            assert_eq!(rows.len(), doc.file.lists.len());
            let (mut d, mut p, mut all) = (0, 0, 0);
            for r in &rows {
                assert_eq!(r.described + r.placeholder + r.undefined, r.item_size, "{}", r.name);
                d += r.described * r.count;
                p += r.placeholder * r.count;
                all += r.item_size * r.count;
            }
            eprintln!("{rel}: {:.1}% described, {:.1}% placeholders (by record bytes)", 100.0 * d as f64 / all as f64, 100.0 * p as f64 / all as f64);
        }
    }

    #[test]
    fn exports_lists_and_search_results() {
        use export::Source;
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let dir = std::env::temp_dir().join(format!("jdide-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let list = dir.join("list3.json");
        let r = doc.export(&Source::List { list: 3 }, true, list.to_str().unwrap()).unwrap();
        assert_eq!(r.records, doc.file.lists[3].count);
        let parsed: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&list).unwrap()).unwrap();
        assert_eq!(parsed["records"].as_array().unwrap().len(), r.records);
        assert!(parsed["records"][0].get("proc_type#label").is_some());
        // Every match of a search, not just the first 500.
        let query = search::Query::Conditions { conditions: vec![search::Condition { field: "proc_type".into(), op: search::Op::HasFlags, value: "16".into() }], match_all: true, list: None };
        let json = dir.join("all.json");
        let r = doc.export(&Source::Search { query }, false, json.to_str().unwrap()).unwrap();
        assert!(r.records > search::LIMIT);
        let parsed: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
        assert_eq!(parsed["elementsVersion"], 156);
        assert_eq!(parsed["records"].as_array().unwrap().len(), r.records);
        assert!(parsed["records"][0]["id"].is_number());
        assert!(parsed["records"][0]["_raw"].is_string());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compares_versions_by_struct_and_id() {
        let (Some(mut a), Some(b)) = (open("Game Dev/JD/1792/gamed/config/elements.data"), open("Games/ForsakenJD/element/data/elements.data")) else { return };
        let t = std::time::Instant::now();
        let s = compare::summary(&a, &b);
        eprintln!("summary in {:?}: v{} vs v{}, {} list pairs, dialogs {:?}", t.elapsed(), s.this.version, s.other.version, s.lists.len(), s.talks);
        for l in s.lists.iter().take(6) {
            eprintln!("  {:30} this {:?} other {:?}: +{} -{} ~{}", l.name, l.this, l.other, l.only_this, l.only_other, l.changed);
        }
        // Equipment pairs by struct even when list numbers differ.
        let equip = s.lists.iter().find(|l| l.struct_name.as_deref() == Some("EQUIPMENT_ESSENCE")).unwrap();
        assert!(equip.this.is_some() && equip.other.is_some());
        let d = compare::list_diff(&a, &b, equip.this, equip.other);
        assert_eq!(d.only_this.len(), equip.only_this);
        if let Some(c) = d.changed.first() {
            eprintln!("  e.g. {} ({}): {:?}", c.this.name, c.this.id, c.fields.iter().take(3).map(|f| (&f.field, &f.this, &f.other)).collect::<Vec<_>>());
            assert!(!c.fields.is_empty());
        }
        let md = compare::markdown(&a, &b, true);
        assert!(md.starts_with("# elements.data changes"));
        // A compatible named field can be copied across versions and undone.
        if let Some((row, other_row, field)) = d.changed.iter().find_map(|c| {
            c.fields.iter().find(|f| f.copyable).map(|f| (c.this.row, c.other_row, f.field.clone()))
        }) {
            let request = compare::CopyRequest {
                this_list: equip.this.unwrap(),
                other_list: equip.other.unwrap(),
                fields: vec![compare::CopyFields { this_row: row, other_row, fields: vec![field.clone()] }],
                records: vec![],
            };
            let state = compare::copy_selection(&mut a, &b, &request).unwrap();
            assert!(state.undo.as_deref().is_some_and(|s| s.starts_with("Copy from compared file")));
            let after = compare::list_diff(&a, &b, equip.this, equip.other);
            assert!(!after.changed.iter().find(|c| c.this.row == row).is_some_and(|c| c.fields.iter().any(|f| f.field == field)));
            a.undo();
        }
        // A file compared with itself has no differences.
        assert!(compare::summary(&a, &a).lists.iter().all(|l| l.only_this + l.only_other + l.changed == 0));
    }

    #[test]
    fn compare_copies_missing_records_only_for_matching_layouts() {
        let (Some(mut a), Some(b)) = (open("Game Dev/JD/zxserver/zgame/gs/config/elements.data"), open("Game Dev/JD/zxserver/zgame/gs/config/elements.data")) else { return };
        let list = 3;
        let before = a.file.lists[list].count;
        let source: Vec<Vec<u8>> = (0..2).map(|row| b.file.record(list, row).unwrap().to_vec()).collect();
        // Make two records genuinely absent from the opened document, before its edit journal begins.
        a.file.remove_record(list, 1);
        a.file.remove_record(list, 0);
        a.edits = edit::Journal::new(a.file.lists.iter().map(|block| block.count));
        let pair = compare::summary(&a, &b).lists.into_iter().find(|p| p.this == Some(list) && p.other == Some(list)).unwrap();
        assert!(pair.can_copy_records_from_other);
        assert_eq!(pair.only_other, 2);
        let request = compare::CopyRequest { this_list: list, other_list: list, fields: vec![], records: vec![0, 1] };
        let state = compare::copy_selection(&mut a, &b, &request).unwrap();
        assert_eq!(a.file.lists[list].count, before);
        assert_eq!(state.added.len(), 2);
        assert_eq!(a.file.record(list, before - 2).unwrap(), source[0]);
        assert_eq!(a.file.record(list, before - 1).unwrap(), source[1]);
        let history = a.history();
        assert_eq!(history[0].records.iter().filter(|r| r.action == "copy").count(), 2);
        let undone = a.undo();
        assert_eq!(a.file.lists[list].count, before - 2);
        assert!(undone.added.is_empty());

        let Some(other_version) = open("Game Dev/JD/1792/gamed/config/elements.data") else { return };
        let mismatch = compare::CopyRequest { this_list: list, other_list: list, fields: vec![], records: vec![0] };
        assert!(compare::copy_selection(&mut a, &other_version, &mismatch).unwrap_err().contains("same elements version"));
    }

    #[test]
    fn edits_undo_redo_and_revert() {
        let Some(mut doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let before = doc.file.data.clone();
        let row = doc.records(3).unwrap().into_iter().find(|r| r.id > 1000).unwrap();
        let price = doc.record(3, row.index).unwrap().nodes.into_iter().find(|n| n.name == "price").unwrap();
        let edit = |v: &str| vec![edit::FieldEdit { off: price.off, value: v.into() }];
        let s = doc.edit(3, row.index, &edit("12345"), "Set price").unwrap();
        assert_eq!(s.changed, vec![(3, row.index)]);
        assert_eq!(s.undo.as_deref(), Some("Set price"));
        let detail = doc.record(3, row.index).unwrap();
        assert_eq!(detail.nodes.iter().find(|n| n.name == "price").unwrap().value.as_deref(), Some("12345"));
        assert!(detail.original.is_some());
        // A bad value changes nothing.
        assert!(doc.edit(3, row.index, &edit("abc"), "Set price").is_err());
        // Undo restores the bytes, redo applies them again.
        let s = doc.undo();
        assert!(s.changed.is_empty() && s.redo.is_some());
        assert_eq!(doc.file.data, before);
        doc.redo();
        assert_ne!(doc.file.data, before);
        // Renaming shows in the record list (caches are refreshed).
        let name = doc.record(3, row.index).unwrap().nodes.into_iter().find(|n| n.name == "name").unwrap();
        doc.edit(3, row.index, &[edit::FieldEdit { off: name.off, value: "Renamed sword".into() }], "Rename").unwrap();
        assert_eq!(doc.records(3).unwrap()[row.index].name, "Renamed sword");
        assert!(doc.find("Renamed sword", 5).total >= 1);
        // Reverting all is one undoable step back to the opened file.
        let s = doc.revert(None, "Revert all");
        assert!(s.changed.is_empty());
        assert_eq!(doc.file.data, before);
        doc.undo();
        assert_ne!(doc.file.data, before);
        // The history lists edits newest first, with their fields; one edit
        // reverts on its own, and a later edit of the same field asks first.
        let price = doc.record(3, row.index).unwrap().nodes.into_iter().find(|n| n.name == "price").unwrap();
        let first = doc.edit(3, row.index, &[edit::FieldEdit { off: price.off, value: "111".into() }], "Set price").unwrap();
        assert!(first.undo.is_some());
        doc.edit(3, row.index, &[edit::FieldEdit { off: name.off, value: "Second name".into() }], "Rename").unwrap();
        let history = doc.history();
        assert_eq!(history[0].label, "Rename");
        let set_price = history.iter().find(|h| h.label == "Set price" && h.records[0].fields.iter().any(|f| f.new == "111")).unwrap();
        assert_eq!(set_price.records[0].fields[0].field, "price");
        let price_id = set_price.id;
        doc.revert_entry(price_id, false).unwrap();
        assert_eq!(doc.record(3, row.index).unwrap().nodes.iter().find(|n| n.name == "name").unwrap().value.as_deref(), Some("Second name"));
        assert_ne!(doc.record(3, row.index).unwrap().nodes.iter().find(|n| n.name == "price").unwrap().value.as_deref(), Some("111"));
        assert_eq!(doc.history().iter().find(|h| h.id == price_id).unwrap().reverted_by.is_some(), true);
        // The revert is no edit of its own in the history (nothing to revert back and forth).
        assert!(doc.history().iter().all(|h| h.reverts.is_none() && !h.label.starts_with("Revert “")));
        // Reverting a reverted edit again does nothing.
        let before = doc.file.data.clone();
        doc.revert_entry(price_id, false).unwrap();
        assert_eq!(doc.file.data, before);
        // Undo takes the revert back: the edit is no longer marked reverted.
        doc.undo();
        assert!(doc.history().iter().find(|h| h.id == price_id).unwrap().reverted_by.is_none());
        doc.redo();
        // A field edited again after the edit being reverted: ask first.
        let renamed = doc.history().iter().find(|h| h.label == "Rename").unwrap().id;
        doc.edit(3, row.index, &[edit::FieldEdit { off: name.off, value: "Third name".into() }], "Rename again").unwrap();
        assert!(doc.revert_entry(renamed, false).unwrap_err().starts_with("CONFLICT"));
        doc.revert_entry(renamed, true).unwrap();
        // Edits survive a schema reload.
        let reloaded = doc.reload(doc.catalog.clone()).unwrap();
        assert_eq!(reloaded.edit_state().changed, vec![(3, row.index)]);
    }

    #[test]
    fn clone_and_delete_records() {
        let Some(mut doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let before = doc.file.data.clone();
        let count = doc.file.lists[3].count;
        let source = doc.records(3).unwrap().into_iter().find(|r| r.id > 1000 && !r.name.is_empty()).unwrap();
        // The new ID is the list's highest plus one, past IDs other lists of
        // the item space (materials, types, services…) already use.
        let next = doc.next_free_id(3).unwrap();
        let max = doc.records(3).unwrap().iter().map(|r| r.id).max().unwrap();
        assert!(next > max);
        let essence: Vec<usize> = (0..doc.file.lists.len()).filter(|&l| doc.lists[l].struct_name.as_deref().is_some_and(|s| refs::registry_space(s) == refs::IdSpace::Essence)).collect();
        let taken: std::collections::HashSet<u32> = essence.iter().flat_map(|&l| doc.records(l).unwrap().into_iter().map(|r| r.id)).collect();
        assert!(!taken.contains(&next));
        assert!((max + 1..next).all(|id| taken.contains(&id)), "only taken IDs are skipped");
        eprintln!("equipment: highest ID {max}, clone gets {next}");
        let s = doc.clone_record(3, source.index).unwrap();
        assert_eq!(s.created, Some((3, count)));
        assert_eq!(s.added, vec![(3, count)]);
        assert_eq!(s.shifts.len(), 1);
        let clone = &doc.records(3).unwrap()[count];
        assert_eq!((clone.id, clone.name.as_str()), (next, source.name.as_str()));
        assert!(doc.record(3, count).unwrap().added);
        // The file is still well-formed: it parses to the same lists, one record more.
        let reparsed = Document::from_bytes("x".into(), doc.file.data.clone(), doc.catalog.clone()).unwrap();
        assert_eq!(reparsed.file.lists[3].count, count + 1);
        assert_eq!(reparsed.file.talk_count, doc.file.talk_count);
        assert_eq!(reparsed.talks().unwrap().len(), doc.talks().unwrap().len());
        // Edit the clone, then delete a record before it: the clone moves up a row.
        let price = doc.record(3, count).unwrap().nodes.into_iter().find(|n| n.name == "price").unwrap();
        doc.edit(3, count, &[edit::FieldEdit { off: price.off, value: "4242".into() }], "Set price").unwrap();
        let s = doc.delete_record(3, 0).unwrap();
        assert_eq!(s.deleted, vec![(3, 1)]);
        assert_eq!(s.added, vec![(3, count - 1)]);
        assert_eq!(doc.records(3).unwrap()[count - 1].id, next);
        assert_eq!(doc.record(3, count - 1).unwrap().nodes.iter().find(|n| n.name == "price").unwrap().value.as_deref(), Some("4242"));
        // Undo brings the deleted record back in place; redo removes it again.
        let s = doc.undo();
        assert!(s.deleted.is_empty());
        assert_eq!(doc.file.lists[3].count, count + 1);
        doc.redo();
        // The history knows the actions.
        let h = doc.history();
        assert_eq!(h[0].records[0].action, "delete");
        assert!(h.iter().any(|e| e.records.iter().any(|r| r.action == "clone" && r.id == next)));
        // Reverting the delete from the history restores the record.
        let delete_id = h[0].id;
        doc.revert_entry(delete_id, false).unwrap();
        assert_eq!(doc.file.lists[3].count, count + 1);
        // Revert all: clones gone, deleted back, changes undone: the file as opened.
        doc.delete_record(3, 5).unwrap();
        let s = doc.revert(None, "Revert all");
        assert!(s.changed.is_empty() && s.added.is_empty() && s.deleted.is_empty());
        assert_eq!(doc.file.data, before);
    }

    #[test]
    fn bulk_edits_search_results() {
        use edit::{BulkEdit, BulkOp};
        use search::{Condition, Op, Query};
        let Some(mut doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let before = doc.file.data.clone();
        let tradeable = Query::Conditions { conditions: vec![Condition { field: "proc_type".into(), op: Op::LacksFlags, value: "16".into() }], match_all: true, list: Some(3) };
        let bulk = |op, field: &str, value: &str| BulkEdit { query: tradeable.clone(), records: None, field: field.into(), op, value: value.into() };
        // A preview changes nothing.
        let plan = doc.bulk_edit(&bulk(BulkOp::SetFlags, "proc_type", "16"), false).unwrap();
        assert!(plan.matched > 100 && plan.changing == plan.matched && plan.failed == 0, "{} {}", plan.matched, plan.changing);
        assert_eq!(doc.file.data, before);
        // Applying flags every record, keeping their other bits; one undo step.
        let first = doc.records(3).unwrap().into_iter().map(|r| r.index).find(|&r| doc.search_limited(&tradeable, usize::MAX).unwrap().hits.iter().any(|h| h.row == r)).unwrap();
        let proc_before: u32 = doc.record(3, first).unwrap().nodes.iter().find(|n| n.name == "proc_type").unwrap().value.as_ref().unwrap().parse().unwrap();
        let done = doc.bulk_edit(&bulk(BulkOp::SetFlags, "proc_type", "16"), true).unwrap();
        assert_eq!(done.state.as_ref().unwrap().changed.len(), plan.changing);
        let proc_after: u32 = doc.record(3, first).unwrap().nodes.iter().find(|n| n.name == "proc_type").unwrap().value.as_ref().unwrap().parse().unwrap();
        assert_eq!(proc_after, proc_before | 16);
        assert_eq!(doc.search_limited(&tradeable, usize::MAX).unwrap().hits.len(), 0);
        assert!(doc.edit_state().undo.unwrap().starts_with("Bulk: proc_type += 16"));
        doc.undo();
        assert_eq!(doc.file.data, before);
        // Arithmetic, with values that do not fit reported, not written.
        let plan = doc.bulk_edit(&bulk(BulkOp::Multiply, "price", "100000000000"), false).unwrap();
        assert!(plan.failed > 0 && plan.samples.iter().any(|s| s.error.as_deref().is_some_and(|e| e.contains("does not fit"))));
        let plan = doc.bulk_edit(&bulk(BulkOp::Add, "price", "5"), false).unwrap();
        let sample = &plan.samples[0];
        assert_eq!(sample.new.parse::<i64>().unwrap(), sample.old.parse::<i64>().unwrap() + 5);
        // Only the records picked.
        let picked: Vec<(usize, usize)> = doc.search_limited(&tradeable, usize::MAX).unwrap().hits.iter().take(3).map(|h| (h.list, h.row)).collect();
        let only = BulkEdit { records: Some(picked.clone()), ..bulk(BulkOp::SetFlags, "proc_type", "16") };
        let done = doc.bulk_edit(&only, true).unwrap();
        assert_eq!((done.matched, done.state.unwrap().changed), (3, picked));
        doc.undo();
        // A name several fields share needs a path.
        let err = doc.bulk_edit(&bulk(BulkOp::Set, "id", "1"), false);
        assert!(err.is_ok() || err.unwrap_err().contains("path"));
    }

    #[test]
    fn client_texts_describe_records() {
        let client = std::path::Path::new("E:/Games/ForsakenJD/element");
        let Some(mut doc) = open("Games/ForsakenJD/element/data/elements.data") else { return };
        if !client.join("configs.pck").exists() {
            return;
        }
        let info = crate::client::inspect(client).unwrap();
        doc.resources = Some(std::sync::Arc::new(crate::client::Resources::new(info)));
        let t = std::time::Instant::now();
        let equipment = doc.records(3).unwrap();
        let described = equipment.iter().filter(|r| doc.game_text(3, r.id).is_some()).count();
        let coloured = equipment.iter().filter(|r| r.color.is_some()).count();
        eprintln!("equipment: {described} of {} described, {coloured} coloured ({:?})", equipment.len(), t.elapsed());
        let table = doc.resources.as_ref().unwrap().table(crate::client::table::ITEM_DESC);
        let entries = table.as_ref().as_ref().unwrap().strings.len();
        let mut per_list: Vec<(usize, usize, usize)> = (0..doc.file.lists.len()).map(|l| (doc.records(l).unwrap().iter().filter(|r| doc.game_text(l, r.id).is_some()).count(), l, doc.file.lists[l].count)).filter(|x| x.0 > 0).collect();
        per_list.sort_by(|a, b| b.cmp(a));
        eprintln!("item_ext_desc: {entries} entries; described per list: {:?}", per_list.iter().take(12).map(|(n, l, c)| format!("{} {n}/{c}", doc.list_name(*l))).collect::<Vec<_>>());
        assert!(per_list.iter().map(|x| x.0).sum::<usize>() > 1000 && coloured > 0);
        let row = equipment.iter().find(|r| doc.game_text(3, r.id).is_some()).unwrap();
        let detail = doc.record(3, row.index).unwrap();
        assert_eq!(detail.game_text.as_ref().unwrap().source, "item_ext_desc.txt");
        let res = doc.resources.as_ref().unwrap();
        eprintln!("skill 1: {:?}, buff 1: {:?}", res.skill_name(1), res.buff_name(1));
        assert!(res.buff_name(1).is_some());
        assert!(res.buff_description(1).is_some());
        assert!(res.skill_description(1124).is_some());
        let task_skill = res.skill_description(366).expect("skill 366 uses its +2 description slot");
        assert!(!task_skill.contains("%%"));
    }

    #[test]
    fn find_matches_ids_then_names() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let row = doc.records(3).unwrap().into_iter().find(|r| r.id != 0 && !r.name.is_empty()).unwrap();
        let by_id = doc.find(&row.id.to_string(), 50);
        assert_eq!(by_id.hits[0].how, "id");
        assert!(by_id.hits.iter().any(|h| h.list == 3 && h.index == row.index));
        let by_name = doc.find(&row.name.to_uppercase(), 50);
        assert!(by_name.total >= 1 && by_name.hits.iter().all(|h| h.how == "name" || h.id == row.id));
        assert!(by_name.hits.iter().any(|h| h.list == 3 && h.index == row.index));
        assert_eq!(doc.find("  ", 50).total, 0);
        if let Some(big) = open("Game Dev/JD/1792/gamed/config/elements.data") {
            let _ = big.find("a", 200);
            let t = std::time::Instant::now();
            let r = big.find("a", 200);
            eprintln!("find over {} lists: {} hits in {:?}", big.file.lists.len(), r.total, t.elapsed());
        }
    }

    #[test]
    fn unknown_version_falls_back_gracefully() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        // Pretend the file is an unknown version: marker tables still apply
        // and names come from size alignment.
        let mut data = doc.file.data.clone();
        data[0] = 0x9d; // v157
        let doc = Document::from_bytes("v157".into(), data, builtin()).unwrap();
        assert_eq!(doc.mode, ParseMode::Markers);
        assert_eq!(doc.file.lists.len(), 193);
        assert!(doc.lists.iter().filter(|r| r.fit == LayoutFit::Borrowed).count() > 180);
        assert_eq!(doc.edit_target(), "v157");
    }

    #[test]
    fn schema_edits_persist_as_user_layouts() {
        let Some(doc) = open("Game Dev/JD/1559/gamed/config/elements.data") else { return };
        let dir = std::env::temp_dir().join(format!("jdide-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let list = 40;
        let size = doc.file.lists[list].item_size;
        let def = ListDef {
            key: None,
            name: "My List".into(),
            struct_name: Some("MY_LIST".into()),
            size: Some(size),
            fields: vec![Field {
                name: "id".into(),
                off: 0,
                t: Ty::U32,
                c: Some("written in a test".into()),
                e: None,
                display: None,
                refs: vec![],
                g: None,
                color: None,
                gc: None,
                when: vec![],
            }],
        };
        def.check().unwrap();

        // Save into the user folder and reload: the edit wins over borrowing.
        let target = doc.edit_target();
        format::save_user_list(&dir.join("layouts"), &target, list, &def, doc.new_target_meta().as_ref()).unwrap();
        let file = format::user_list_path(&dir.join("layouts"), &target, list);
        assert!(file.ends_with(format!("{target}/list_{list}.json")));
        assert!(file.exists());
        let doc = doc.reload(Arc::new(Catalog::load(Some(&dir)))).unwrap();
        assert_eq!(doc.list_name(list), "My List");
        assert_eq!(doc.lists[list].fit, LayoutFit::Exact);
        assert!(doc.summary().lists[list].custom);
        assert!(doc.list_schema(list).unwrap().custom);

        // Other lists still come from the built-in layout.
        assert_eq!(doc.lists[0].fit, LayoutFit::Exact);
        assert!(!doc.summary().lists[0].custom);

        // Reverting removes the user file again.
        format::delete_user_list(&dir.join("layouts"), &target, list).unwrap();
        assert!(!file.exists());
        let doc = doc.reload(Arc::new(Catalog::load(Some(&dir)))).unwrap();
        assert!(!doc.summary().lists[list].custom);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_offers_the_same_list_from_other_versions() {
        let Some(doc) = open("Game Dev/JD/1559/gamed/config/elements.data") else { return };
        let list = 3; // Equipment Essence
        let candidates = doc.import_candidates(list).unwrap();
        assert!(candidates.iter().all(|c| c.layout_id != "v158"));
        assert!(candidates.iter().all(|c| !c.def.fields.is_empty()));
        let v156 = candidates.iter().find(|c| c.layout_id == "v156").unwrap();
        assert_eq!(v156.struct_name.as_deref(), Some("EQUIPMENT_ESSENCE"));
        assert_eq!((v156.size, v156.item_size), (644, 708));
        let versions: Vec<u32> = candidates.iter().map(|c| c.version).collect();
        assert!(versions.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn addon_params_switch_to_float_by_type() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let addons = doc.by_struct["EQUIPMENT_ADDON"][0];
        let rows = doc.records(addons).unwrap();
        let param1_of = |want_type: &str| {
            rows.iter().find_map(|r| {
                let d = doc.record(addons, r.index).unwrap();
                let ty = d.nodes.iter().find(|n| n.name == "type")?;
                (ty.value.as_deref() == Some(want_type))
                    .then(|| d.nodes.into_iter().find(|n| n.name == "param1").unwrap())
            })
        };
        // 7 is a rate (float), 1 is flat health (int).
        let rate = param1_of("7").expect("an addon of type 7");
        assert_eq!(rate.ty, "float");
        assert_eq!(rate.cond.as_deref(), Some("type = 7 → float"));
        let health = param1_of("1").expect("an addon of type 1");
        assert_eq!(health.ty, "int32");
    }

    #[test]
    fn edits_for_an_unknown_version_create_a_user_layout() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let mut data = doc.file.data.clone();
        data[0] = 0x9d; // v157: no built-in layout
        let doc = Document::from_bytes("v157".into(), data, builtin()).unwrap();
        let dir = std::env::temp_dir().join(format!("jdide-test-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let def = doc.list_schema(0).unwrap().def.unwrap();
        let meta = doc.new_target_meta().expect("a new layout");
        assert_eq!(meta.id, "v157");
        format::save_user_list(&dir.join("layouts"), "v157", 0, &def, Some(&meta)).unwrap();
        assert!(dir.join("layouts/v157/layout.json").exists());

        // The new layout's marker table now reads the file as its own version.
        let doc = doc.reload(Arc::new(Catalog::load(Some(&dir)))).unwrap();
        assert_eq!(doc.mode, ParseMode::Layout);
        assert_eq!(doc.lists[0].fit, LayoutFit::Exact);
        assert!(doc.summary().layout_custom);

        // Removing its last list removes the user-only layout.
        format::delete_user_list(&dir.join("layouts"), "v157", 0).unwrap();
        assert!(!dir.join("layouts/v157").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn client_resources_give_icons_and_paths() {
        let client = std::path::Path::new("E:/Games/ForsakenJD");
        let Some(mut doc) = open("Games/ForsakenJD/element/data/elements.data") else { return };
        let Ok(info) = crate::client::inspect(client) else { return };
        doc.resources = Some(Arc::new(crate::client::Resources::new(info)));
        let equipment = doc.by_struct["EQUIPMENT_ESSENCE"][0];
        let rows = doc.records(equipment).unwrap();
        let with_icons = rows.iter().filter(|r| r.icon.is_some()).count();
        assert!(with_icons * 10 > rows.len() * 9, "{with_icons} of {} rows have icons", rows.len());

        let row = rows.iter().find(|r| r.icon.is_some()).unwrap();
        let detail = doc.record(equipment, row.index).unwrap();
        assert_eq!(detail.icon, row.icon);
        let icon = detail.nodes.iter().find(|n| n.display.as_deref() == Some("icon")).unwrap();
        assert_eq!(icon.icon, row.icon);
        assert!(icon.hint.as_deref().is_some_and(|p| p.to_lowercase().ends_with(".dds")), "{:?}", icon.hint);
        let png = doc.resources.as_ref().unwrap().item_icon_png(row.icon.unwrap()).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn hdn_npc_profile_is_a_previewable_image() {
        let client = std::path::Path::new("E:/Games/Elite Jade Dynasty - HDN");
        let Some(mut doc) = open("Games/Elite Jade Dynasty - HDN/element/data/elements.data") else { return };
        let Ok(info) = crate::client::inspect(client) else { return };
        doc.resources = Some(Arc::new(crate::client::Resources::new(info)));
        let row = doc.records(34).unwrap().into_iter().find(|row| row.id == 16).expect("NPC 16");
        let detail = doc.record(34, row.index).unwrap();
        let profile = detail.nodes.iter().find(|node| node.name.eq_ignore_ascii_case("profile_path_id")).expect("Profile_Path_ID");
        assert_eq!(profile.value.as_deref(), Some("7318"));
        assert_eq!(profile.display.as_deref(), Some("image"));
        assert_eq!(profile.image, Some(7318));
        assert!(profile.hint.as_deref().is_some_and(|path| path.to_lowercase().starts_with("surfaces\\npcimg\\") && path.to_lowercase().ends_with(".tga")), "{:?}", profile.hint);
    }

    #[test]
    fn referenced_by_finds_shops_and_recipes_but_not_other_id_spaces() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        let equipment = doc.by_struct["EQUIPMENT_ESSENCE"][0];
        let refs = doc.referenced_by(equipment, 100).unwrap();
        assert_eq!(refs.id, 387);
        let lists: Vec<&str> = refs.referrers.iter().map(|r| doc.lists[r.list].struct_name.as_deref().unwrap_or("")).collect();
        assert!(lists.contains(&"NPC_SELL_SERVICE"), "{lists:?}");
        assert!(lists.contains(&"RECIPE_ESSENCE"), "{lists:?}");
        // Task, recipe-roll and config IDs live in other ID spaces.
        assert!(refs.referrers.iter().all(|r| !r.field.contains("task") && !r.field.contains("config") && !r.field.contains("id_recipe")), "{:?}", refs.referrers);
        let shop = refs.referrers.iter().find(|r| r.field.contains("id_goods")).unwrap();
        assert_eq!(shop.how, "id");

        // Declared refs: an addon is referenced by the equipment that has it.
        let addons = doc.by_struct["EQUIPMENT_ADDON"][0];
        let rows = doc.records(addons).unwrap();
        let used = rows
            .iter()
            .map(|r| doc.referenced_by(addons, r.index).unwrap())
            .find(|r| r.referrers.iter().any(|x| x.how == "declared"))
            .expect("an addon used by some item");
        assert!(used.referrers.iter().any(|x| x.field.starts_with("id_addon")));
    }

    #[test]
    fn invalid_definitions_are_rejected() {
        let field = |name: &str, off| Field { name: name.into(), off, t: Ty::I32, c: None, e: None, display: None, refs: vec![], g: None, color: None, gc: None, when: vec![] };
        let def = |fields, size| ListDef { key: None, name: "L".into(), struct_name: None, size, fields };
        assert!(def(vec![field("a", 0), field("a", 4)], Some(8)).check().is_err());
        assert!(def(vec![field("a", 0), field("b", 4)], Some(6)).check().is_err());
        assert!(def(vec![field("a", 0)], None).check().is_err());
        assert!(def(vec![field("a", 0), field("b", 4)], Some(8)).check().is_ok());
    }
}


