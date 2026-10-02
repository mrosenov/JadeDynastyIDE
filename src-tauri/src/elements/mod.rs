pub mod decode;
pub mod profile;
pub mod reader;

use serde::Serialize;

use decode::{decode_record, gap_node, guess_name, read_wstr, Node};
use profile::{ListDef, Profile, ProfileMatch, Ty};
use reader::{ElementsFile, Segment};

/// How well the profile describes a list's records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutFit {
    /// Layout made for this exact version and record size.
    Exact,
    /// Layout from a neighbouring version with the same record size.
    Approx,
    /// Record is larger than the layout; the tail is shown as unknown bytes.
    Partial,
    /// No usable layout.
    None,
}

pub struct Document {
    pub path: String,
    pub file: ElementsFile,
    pub profile: Option<ProfileMatch>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSummary {
    pub index: usize,
    pub name: String,
    pub key: Option<String>,
    pub struct_name: Option<String>,
    /// Name comes from a neighbouring version's profile, by position.
    pub approx_name: bool,
    pub item_size: usize,
    pub count: usize,
    pub offset: usize,
    pub layout: LayoutFit,
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
    pub layout_signature: String,
    pub profile_version: Option<u32>,
    pub profile_source: Option<String>,
    pub profile_exact: bool,
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
    pub layout_size: Option<usize>,
    pub bytes: Vec<u8>,
    pub nodes: Vec<Node>,
}

impl Document {
    pub fn open(path: String) -> Result<Self, String> {
        let data = std::fs::read(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
        let file = ElementsFile::parse(data).map_err(|e| format!("Not a readable elements.data: {e}"))?;
        let profile = profile::select(file.version(), &file.layout_signature());
        Ok(Self { path, file, profile })
    }

    fn profile(&self) -> Option<&'static Profile> {
        self.profile.as_ref().map(|m| m.profile)
    }

    fn list_def(&self, list: usize) -> Option<&'static ListDef> {
        self.profile()?.lists.get(list)
    }

    fn layout_fit(&self, list: usize) -> LayoutFit {
        let (Some(m), Some(def)) = (&self.profile, self.list_def(list)) else {
            return LayoutFit::None;
        };
        let item_size = self.file.lists[list].item_size;
        match def.size {
            _ if def.fields.is_empty() => LayoutFit::None,
            Some(size) if size == item_size && m.exact => LayoutFit::Exact,
            Some(size) if size == item_size => LayoutFit::Approx,
            Some(size) if size < item_size => LayoutFit::Partial,
            _ => LayoutFit::None,
        }
    }

    pub fn summary(&self) -> FileSummary {
        let exact = self.profile.as_ref().is_some_and(|m| m.exact);
        let lists = self
            .file
            .lists
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let def = self.list_def(index);
                ListSummary {
                    index,
                    name: def.map(|d| d.name.clone()).unwrap_or_else(|| format!("List {}", index + 1)),
                    key: def.and_then(|d| d.key.clone()),
                    struct_name: def.and_then(|d| d.struct_name.clone()),
                    approx_name: def.is_some() && !exact,
                    item_size: block.item_size,
                    count: block.count,
                    offset: block.header_offset,
                    layout: self.layout_fit(index),
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
            layout_signature: self.file.layout_signature(),
            profile_version: self.profile().map(|p| p.version),
            profile_source: self.profile().map(|p| p.source.clone()),
            profile_exact: exact,
            talk_count: self.file.talk_count,
            lists,
            segments: self.file.segments.clone(),
        }
    }

    /// Offset of the record's name string, if the layout has one.
    fn name_offset(&self, list: usize) -> Option<(usize, usize)> {
        if self.layout_fit(list) == LayoutFit::None {
            return None;
        }
        let fields = &self.list_def(list)?.fields;
        let wstr = |f: &&profile::Field| matches!(f.t, Ty::Wstr { .. });
        let field = fields
            .iter()
            .filter(wstr)
            .find(|f| f.name.eq_ignore_ascii_case("name"))
            .or_else(|| fields.iter().find(wstr))?;
        Some((field.off, field.t.size()))
    }

    pub fn records(&self, list: usize) -> Result<Vec<RecordRow>, String> {
        let block = self.file.lists.get(list).ok_or("No such list")?;
        let name_at = self.name_offset(list);
        Ok((0..block.count)
            .map(|index| {
                let bytes = self.file.record(list, index).unwrap();
                let id = bytes.get(0..4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0);
                let name = match name_at {
                    Some((off, size)) if off + size <= bytes.len() => read_wstr(&bytes[off..off + size]),
                    _ => guess_name(bytes).unwrap_or_default(),
                };
                RecordRow { index, id, name }
            })
            .collect())
    }

    pub fn record(&self, list: usize, index: usize) -> Result<RecordDetail, String> {
        let bytes = self.file.record(list, index).ok_or("No such record")?;
        let layout = self.layout_fit(list);
        let def = self.list_def(list);
        let nodes = match (layout, def, self.profile()) {
            (LayoutFit::None, ..) | (_, None, _) | (_, _, None) => vec![gap_node(bytes, 0, bytes.len())],
            (_, Some(def), Some(profile)) => decode_record(bytes, &def.fields, profile),
        };
        Ok(RecordDetail {
            list,
            index,
            file_offset: self.file.record_offset(list, index).unwrap(),
            layout,
            layout_size: def.and_then(|d| d.size),
            bytes: bytes.to_vec(),
            nodes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(rel: &str) -> Option<Document> {
        let root = std::env::var("JDIDE_SAMPLES").unwrap_or_else(|_| "E:/Game Dev/JD".into());
        let path = format!("{root}/{rel}");
        std::path::Path::new(&path).exists().then(|| Document::open(path).unwrap())
    }

    #[test]
    fn v156_layouts_are_exact_for_every_list() {
        let Some(doc) = open("zxserver/zgame/gs/config/elements.data") else { return };
        let summary = doc.summary();
        assert!(summary.profile_exact);
        assert!(summary.lists.iter().all(|l| l.layout == LayoutFit::Exact));
        let equipment = summary.lists.iter().position(|l| l.key.as_deref() == Some("equipment_essence")).unwrap();
        let rows = doc.records(equipment).unwrap();
        assert!(rows.iter().any(|r| !r.name.is_empty()));
    }

    #[test]
    fn v112_uses_jade_editor_names_and_schemas() {
        let Some(doc) = open("Tools/JadeEditorFOX/tests/elements - Copy.data") else { return };
        let summary = doc.summary();
        assert_eq!(summary.lists[0].name, "AddedAttribute");
        assert_eq!(summary.lists[0].count, 3366);
        let row = doc.records(0).unwrap().into_iter().find(|r| r.id == 1752).unwrap();
        assert_eq!(row.name, "法宝技能_真山河扇");
        let detail = doc.record(0, row.index).unwrap();
        let ty = detail.nodes.iter().find(|n| n.name == "Type").unwrap();
        assert_eq!(ty.value.as_deref(), Some("38"));
        assert_eq!(ty.hint.as_deref(), Some("Bonus_Skill"));
    }

    #[test]
    fn newer_versions_borrow_v156_names() {
        let Some(doc) = open("1792/gamed/config/elements.data") else { return };
        let summary = doc.summary();
        assert_eq!(summary.profile_version, Some(156));
        assert!(!summary.profile_exact);
        assert_eq!(summary.lists[0].layout, LayoutFit::Approx);
        assert_eq!(summary.lists[318].name, "List 319");
        assert_eq!(summary.lists[3].layout, LayoutFit::Partial);
    }

    #[test]
    fn unknown_layouts_fall_back_to_raw() {
        let Some(doc) = open("1792/gamed/config/c01/elements.data") else { return };
        let summary = doc.summary();
        assert_eq!(summary.profile_version, None);
        let detail = doc.record(0, 0).unwrap();
        assert_eq!(detail.layout, LayoutFit::None);
        assert!(detail.nodes[0].unknown);
    }
}
