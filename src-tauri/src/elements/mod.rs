pub mod align;
pub mod decode;
pub mod format;
pub mod reader;

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;

use align::{align, Match};
use decode::{decode_record, gap_node, guess_name, read_wstr, Annotation, Node};
use format::{catalog, Field, Layout, ListDef, Marker, MarkerKind, Ty};
use reader::{ElementsFile, Segment, SegmentKind};

/// Where a list's definition comes from and how well it fits the records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    /// Definition used for fields, and the layout it belongs to.
    def: Option<(&'static Layout, &'static ListDef)>,
    fit: LayoutFit,
}

pub struct Document {
    pub path: String,
    pub file: ElementsFile,
    mode: ParseMode,
    primary: Option<&'static Layout>,
    markers_from: Option<&'static Layout>,
    lists: Vec<Resolved>,
    by_struct: HashMap<String, Vec<usize>>,
    ids: Vec<OnceLock<HashMap<u32, usize>>>,
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
}

/// How well a layout describes a file parsed with its marker table.
fn score(layout: &Layout, file: &ElementsFile) -> i64 {
    let mut score = 0;
    for (i, block) in file.lists.iter().enumerate() {
        match layout.list(i).and_then(|d| d.size) {
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
fn usable(def: &ListDef, size: usize) -> bool {
    def.has_fields() && def.size.is_some_and(|s| s <= size)
}

impl Document {
    pub fn open(path: String) -> Result<Self, String> {
        let data = std::fs::read(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
        Self::from_bytes(path, data)
    }

    pub fn from_bytes(path: String, data: Vec<u8>) -> Result<Self, String> {
        if data.len() < 8 {
            return Err("File is too small to be elements.data".into());
        }
        let version = u32::from_le_bytes(data[0..4].try_into().unwrap()) & 0xffff;
        let cat = catalog();

        // 1. A layout made for this version, the best-fitting one if several.
        let mut best: Option<(i64, &'static Layout, ElementsFile)> = None;
        for layout in cat.layouts.iter().filter(|l| l.version == version) {
            if let Ok(file) = ElementsFile::parse_with(data.clone(), &layout.markers) {
                let s = score(layout, &file);
                if best.as_ref().is_none_or(|(b, ..)| s > *b) {
                    best = Some((s, layout, file));
                }
            }
        }
        let (file, mode, primary, markers_from) = if let Some((_, layout, file)) = best {
            (file, ParseMode::Layout, Some(layout), Some(layout))
        } else if let Some((layout, file)) = cat
            .marker_tables(version)
            .into_iter()
            .filter(|l| l.version != version)
            .find_map(|l| ElementsFile::parse_with(data.clone(), &l.markers).ok().map(|f| (l, f)))
        {
            // 2. Another version's marker table that fits exactly.
            (file, ParseMode::Markers, None, Some(layout))
        } else {
            // 3. Recognise segments by content.
            let file = ElementsFile::parse_detect(data).map_err(|e| format!("Not a readable elements.data: {e}"))?;
            (file, ParseMode::Detected, None, None)
        };

        let lists = Self::resolve(&file, primary);
        let mut by_struct: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in lists.iter().enumerate() {
            if let Some(s) = &r.struct_name {
                by_struct.entry(s.clone()).or_default().push(i);
            }
        }
        let ids = (0..file.lists.len()).map(|_| OnceLock::new()).collect();
        Ok(Self { path, file, mode, primary, markers_from, lists, by_struct, ids })
    }

    /// Picks, per list: the file's own layout, else a definition borrowed
    /// from another layout by size alignment, else whatever name is known.
    fn resolve(file: &ElementsFile, primary: Option<&'static Layout>) -> Vec<Resolved> {
        let cat = catalog();
        let sizes = file.item_sizes();
        let markers = markers_of(file);
        let version = file.version();

        let mut donors: Vec<&'static Layout> =
            cat.layouts.iter().filter(|l| primary.is_none_or(|p| p.id != l.id)).collect();
        donors.sort_by_key(|l| (l.version.abs_diff(version), l.id.clone()));
        let alignments: Vec<Vec<Option<Match>>> = donors.iter().map(|d| align(&sizes, &markers, d)).collect();

        let donor_def = |i: usize, exact: bool| {
            donors.iter().zip(&alignments).find_map(|(layout, matches)| {
                let m = matches[i]?;
                if matches!(m, Match::Exact(_)) != exact {
                    return None;
                }
                let def = layout.list(m.donor())?;
                (usable(def, sizes[i]) && !def.is_placeholder()).then_some((*layout, def))
            })
        };

        (0..sizes.len())
            .map(|i| {
                let size = sizes[i];
                let own = primary.and_then(|l| l.list(i).map(|d| (l, d)));
                let own_real = own.filter(|(_, d)| !d.is_placeholder());

                let (def, fit) = if let Some((l, d)) = own_real.filter(|(_, d)| usable(d, size)) {
                    let fit = if d.size == Some(size) { LayoutFit::Exact } else { LayoutFit::Partial };
                    (Some((l, d)), fit)
                } else if let Some(found) = donor_def(i, true) {
                    (Some(found), LayoutFit::Borrowed)
                } else if let Some(found) = donor_def(i, false) {
                    (Some(found), LayoutFit::Grown)
                } else if let Some((l, d)) = own.filter(|(_, d)| usable(d, size)) {
                    let fit = if d.size == Some(size) { LayoutFit::Exact } else { LayoutFit::Partial };
                    (Some((l, d)), fit)
                } else {
                    let fit = if own.is_some() { LayoutFit::Named } else { LayoutFit::None };
                    (None, fit)
                };

                // The file's own (non-placeholder) name wins over a borrowed one.
                let named = own_real.or(def).or(own).map(|(_, d)| d);
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

    fn list_name(&self, list: usize) -> String {
        self.lists[list].name.clone().unwrap_or_else(|| format!("List {list}"))
    }

    /// Offset and size of the record's name string, if the definition has one.
    fn name_field(&self, list: usize) -> Option<(usize, usize)> {
        let (_, def) = self.lists[list].def?;
        let wstr = |f: &&Field| matches!(f.t, Ty::Wstr { .. });
        let field = def
            .fields
            .iter()
            .filter(wstr)
            .find(|f| f.name.eq_ignore_ascii_case("name"))
            .or_else(|| def.fields.iter().find(wstr))?;
        Some((field.off, field.t.size()))
    }

    fn record_name(&self, list: usize, bytes: &[u8], name_at: Option<(usize, usize)>) -> String {
        match name_at {
            Some((off, size)) if off + size <= bytes.len() => read_wstr(&bytes[off..off + size]),
            _ => guess_name(bytes).unwrap_or_default(),
        }
        .chars()
        .take(if list == usize::MAX { 0 } else { 64 })
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
                ListSummary {
                    index,
                    name: self.list_name(index),
                    key: r.key.clone(),
                    struct_name: r.struct_name.clone(),
                    item_size: block.item_size,
                    count: block.count,
                    offset: block.header_offset,
                    layout: r.fit,
                    layout_id: r.def.map(|(l, _)| l.id.clone()),
                    layout_size: r.def.and_then(|(_, d)| d.size),
                }
            })
            .collect();
        FileSummary {
            path: self.path.clone(),
            file_size: self.file.data.len(),
            version: self.file.version(),
            raw_version: self.file.raw_version,
            timestamp: self.file.timestamp,
            exporter: self.file.exporter.clone(),
            parse_mode: self.mode,
            layout_id: self.primary.map(|l| l.id.clone()),
            layout_source: self.primary.map(|l| l.source.clone()),
            layout_unverified: self.primary.is_some_and(|l| l.list_count_unverified),
            markers_from: self.markers_from.map(|l| l.id.clone()),
            talk_count: self.file.talk_count,
            lists,
            segments: self.file.segments.clone(),
        }
    }

    pub fn records(&self, list: usize) -> Result<Vec<RecordRow>, String> {
        let block = self.file.lists.get(list).ok_or("No such list")?;
        let name_at = self.name_field(list);
        Ok((0..block.count)
            .map(|index| {
                let bytes = self.file.record(list, index).unwrap();
                RecordRow { index, id: Self::record_id(bytes), name: self.record_name(list, bytes, name_at) }
            })
            .collect())
    }

    /// Enum labels and cross-list references for a field value.
    fn annotate(&self, layout: &Layout, field: &Field, value: i64) -> Annotation {
        let mut a = Annotation::default();
        if let Some(set) = field.e.as_deref().and_then(|key| catalog().enum_set(layout, key)) {
            a.hint = set.label_for(value);
            return a;
        }
        if field.refs.is_empty() || value <= 0 || value > u32::MAX as i64 {
            return a;
        }
        let targets: Vec<usize> = field.refs.iter().filter_map(|s| self.by_struct.get(s)).flatten().copied().collect();
        for &list in &targets {
            if let Some(&row) = self.id_index(list).get(&(value as u32)) {
                let bytes = self.file.record(list, row).unwrap();
                let name = self.record_name(list, bytes, self.name_field(list));
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

    pub fn record(&self, list: usize, index: usize) -> Result<RecordDetail, String> {
        let bytes = self.file.record(list, index).ok_or("No such record")?;
        let r = self.lists.get(list).ok_or("No such list")?;
        let nodes = match r.def {
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
            layout: r.fit,
            layout_id: r.def.map(|(l, _)| l.id.clone()),
            layout_size: r.def.and_then(|(_, d)| d.size),
            bytes: bytes.to_vec(),
            nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real files are not checked in. Paths are relative to JDIDE_SAMPLES
    /// (default `E:/`); missing samples are skipped with a note.
    fn open(rel: &str) -> Option<Document> {
        let root = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/".into());
        let path = format!("{root}/{rel}");
        if !std::path::Path::new(&path).exists() {
            eprintln!("skipping {rel}: sample not found");
            return None;
        }
        Some(Document::open(path).unwrap())
    }

    fn tally(doc: &Document) -> HashMap<LayoutFit, usize> {
        let mut t = HashMap::new();
        for r in &doc.lists {
            *t.entry(r.fit).or_default() += 1;
        }
        t
    }

    impl std::hash::Hash for LayoutFit {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            (*self as u8).hash(state);
        }
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
        assert_eq!(doc.primary.unwrap().id, "v156");
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
        assert_eq!(ty.hint.as_deref(), Some("Bonus_Skill"));
    }

    #[test]
    fn unknown_version_falls_back_gracefully() {
        let Some(doc) = open("Game Dev/JD/zxserver/zgame/gs/config/elements.data") else { return };
        // Pretend the file is an unknown version: marker tables still apply
        // and names come from size alignment.
        let mut data = doc.file.data.clone();
        data[0] = 0x9d; // v157
        let doc = Document::from_bytes("v157".into(), data).unwrap();
        assert_eq!(doc.mode, ParseMode::Markers);
        assert_eq!(doc.file.lists.len(), 193);
        assert!(doc.lists.iter().filter(|r| r.fit == LayoutFit::Borrowed).count() > 180);
    }
}
