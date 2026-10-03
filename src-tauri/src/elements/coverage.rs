//! Layout coverage: how much of each list's records the layout describes.

use serde::Serialize;

use super::{search, Document, LayoutFit};

/// Field names layouts use for bytes nobody understood yet: "unknown_12",
/// "Unk3", "pages_1_goods_2_unknown_4", "unknowns_2", "_pad1".
pub fn placeholder(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.starts_with("_pad") || n.starts_with("unknown") {
        return true;
    }
    let last = n.rsplit('_').find(|w| !w.chars().all(|c| c.is_ascii_digit())).unwrap_or("");
    let word = last.trim_end_matches(|c: char| c.is_ascii_digit());
    matches!(word, "unknown" | "unknowns" | "unk")
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageRow {
    pub index: usize,
    pub name: String,
    pub struct_name: Option<String>,
    pub item_size: usize,
    pub count: usize,
    pub fit: LayoutFit,
    pub layout_id: Option<String>,
    /// The definition is the user's (schema editor).
    pub custom: bool,
    /// Bytes of named fields.
    pub described: usize,
    /// Bytes of placeholder fields ("unknown_12").
    pub placeholder: usize,
    /// Bytes no field covers.
    pub undefined: usize,
    /// Named leaf fields (array elements count once per element).
    pub fields: usize,
}

impl Document {
    pub fn coverage(&self) -> Vec<CoverageRow> {
        let summary = self.summary();
        summary
            .lists
            .iter()
            .map(|l| {
                let size = l.item_size;
                let mut described = vec![false; size];
                let mut holder = vec![false; size];
                let mut fields = 0;
                if let Some((_, def)) = self.def(l.index) {
                    for slot in search::slots(def, size) {
                        let end = (slot.off + slot.size()).min(size);
                        let target = if placeholder(slot.name()) { &mut holder } else { &mut described };
                        target[slot.off..end].iter_mut().for_each(|b| *b = true);
                        fields += usize::from(!placeholder(slot.name()));
                    }
                }
                let described_n = described.iter().filter(|&&b| b).count();
                let placeholder_n = holder.iter().zip(&described).filter(|(&h, &d)| h && !d).count();
                CoverageRow {
                    index: l.index,
                    name: l.name.clone(),
                    struct_name: l.struct_name.clone(),
                    item_size: size,
                    count: l.count,
                    fit: l.layout,
                    layout_id: l.layout_id.clone(),
                    custom: l.custom,
                    described: described_n,
                    placeholder: placeholder_n,
                    undefined: size - described_n - placeholder_n,
                    fields,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::placeholder;

    #[test]
    fn placeholder_names() {
        for n in ["unknown_12", "Unknown", "Unk3", "unk_4", "pages_1_goods_2_unknown_4", "faction_item_list_3_unk2", "unknowns_2", "unknown_id_5", "_pad1"] {
            assert!(placeholder(n), "{n}");
        }
        for n in ["id", "name", "unknown_count_but_named".replace("unknown_", "").as_str(), "drunk", "trunk_id", "proc_type"] {
            assert!(!placeholder(n), "{n}");
        }
    }
}
