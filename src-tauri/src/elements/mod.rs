pub mod align;
pub mod decode;
pub mod format;
pub mod reader;
pub mod refs;

use std::collections::HashMap;
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
    /// Integer fields that may hold other records' IDs (for "referenced by").
    sites: OnceLock<Vec<refs::Site>>,
    /// The game client's paths and icons, when a client folder is set.
    pub resources: Option<Arc<Resources>>,
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
        let data = std::fs::read(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
        Self::from_bytes(path, data, catalog)
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
        Ok(Self { path, file, catalog, mode, primary, markers_from, lists, by_struct, ids, sites: OnceLock::new(), resources: None })
    }

    /// Re-reads the same bytes with a new catalog (after a schema edit).
    pub fn reload(&self, catalog: Arc<Catalog>) -> Result<Self, String> {
        let mut doc = Self::from_bytes(self.path.clone(), self.file.data.clone(), catalog)?;
        doc.resources = self.resources.clone();
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
        // Path and icon fields hold path.data IDs: show the client's path.
        if let (Some(display @ ("path" | "icon")), Some(res)) = (field.display.as_deref(), &self.resources) {
            if value > 0 && value <= u32::MAX as i64 {
                let id = value as u32;
                if let Some(path) = res.path(id) {
                    a.hint = Some(path.to_string());
                    if display == "icon" && res.item_icon(id).is_some() {
                        a.icon = Some(id);
                    }
                } else if res.paths().is_ok() {
                    a.hint = Some("not in path.data".into());
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
        })
    }

    pub fn record(&self, list: usize, index: usize) -> Result<RecordDetail, String> {
        let r = self.lists.get(list).ok_or("No such list")?;
        let def = self.def(list).map(|(l, d)| (Some(l), d));
        self.detail(list, index, def, r.fit)
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
        let field = |name: &str, off| Field { name: name.into(), off, t: Ty::I32, c: None, e: None, display: None, refs: vec![], g: None, when: vec![] };
        let def = |fields, size| ListDef { key: None, name: "L".into(), struct_name: None, size, fields };
        assert!(def(vec![field("a", 0), field("a", 4)], Some(8)).check().is_err());
        assert!(def(vec![field("a", 0), field("b", 4)], Some(6)).check().is_err());
        assert!(def(vec![field("a", 0)], None).check().is_err());
        assert!(def(vec![field("a", 0), field("b", 4)], Some(8)).check().is_ok());
    }
}

