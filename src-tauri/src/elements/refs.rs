//! "Referenced by": which records of the file point at a record's ID.
//!
//! Two kinds of reference sites are collected from the list definitions:
//!
//! - declared: integer fields whose `refs` name the target list's struct;
//! - by ID: integer fields whose name says they hold an ID (`id_goods`,
//!   `id_to_make`, `item_id`, …). IDs are only unique within an ID space
//!   (items, addons, recipes, configs, tasks, …), so such a field only counts
//!   when the space its name implies matches the target list's space.

use serde::Serialize;

use super::format::{Field, Ty};

/// The ID pools the client registers records in (simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IdSpace {
    /// Items, monsters, NPCs, mines… (the "essence" pool).
    Essence,
    Addon,
    Recipe,
    Config,
    /// Major/sub types of items.
    Type,
    /// NPC services.
    Service,
    /// Spaces with no list in elements.data (tasks, skills, talks, …).
    Other,
}

/// The space a list's records live in, from its struct name.
pub fn list_space(struct_name: &str) -> IdSpace {
    let s = struct_name.to_ascii_uppercase();
    if s.ends_with("_ADDON") {
        IdSpace::Addon
    } else if s == "RECIPE_ESSENCE" {
        IdSpace::Recipe
    } else if s.ends_with("_CONFIG") {
        IdSpace::Config
    } else if s.ends_with("_TYPE") {
        IdSpace::Type
    } else if s.contains("_SERVICE") {
        IdSpace::Service
    } else {
        IdSpace::Essence
    }
}

/// The ID space the file loader registers a list's records in, per
/// `elementdataman::setup_hash_map` (client/server source): types and services
/// share the essence space, recipe types the recipe space, and two configs
/// live among the essences. Structs it does not know fall back to the name.
pub fn registry_space(struct_name: &str) -> IdSpace {
    const ESSENCE_CONFIGS: [&str; 2] = ["WAR_ROLE_CONFIG", "ITEM_TRADE_CONFIG"];
    let s = struct_name.to_ascii_uppercase();
    if s.ends_with("_ADDON") {
        IdSpace::Addon
    } else if matches!(s.as_str(), "RECIPE_ESSENCE" | "RECIPE_MAJOR_TYPE" | "RECIPE_SUB_TYPE") {
        IdSpace::Recipe
    } else if matches!(s.as_str(), "RUNE_COMB_PROPERTY" | "UPGRADE_EQUIP_CONFIG_1") || (s.ends_with("_CONFIG") && !ESSENCE_CONFIGS.contains(&s.as_str())) {
        IdSpace::Config
    } else {
        IdSpace::Essence
    }
}

/// Lowercase words of a field name: "id_goods" → [id, goods], "IconPathID" → [icon, path, id].
fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let boundary = c.is_ascii_uppercase()
            && i > 0
            && (chars[i - 1].is_ascii_lowercase() || chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()) && chars[i - 1].is_ascii_uppercase());
        if !c.is_ascii_alphanumeric() || boundary {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            if !c.is_ascii_alphanumeric() {
                continue;
            }
        }
        word.push(c.to_ascii_lowercase());
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Whether a field name says it holds an ID: one of its words is "id",
/// "ids" or "id" with a number ("id2").
pub fn id_like(name: &str) -> bool {
    words(name).iter().any(|w| {
        w == "id" || w == "ids" || (w.starts_with("id") && w.len() > 2 && w[2..].chars().all(|c| c.is_ascii_digit()))
    })
}

/// The space an ID field points into, from its name.
pub fn field_space(name: &str) -> IdSpace {
    let n = words(name).join("_");
    let has = |w: &str| n.contains(w);
    if has("task") || has("skill") || has("talk") || has("dialog") || has("title") || has("faction") || has("region") || has("map") || has("world") || has("instance") || has("controller") || has("pet_") {
        IdSpace::Other
    } else if has("recipe") {
        IdSpace::Recipe
    } else if has("config") || has("buff_area") || has("area") {
        IdSpace::Config
    } else if has("addon") || has("prop") {
        IdSpace::Addon
    } else if has("type") {
        IdSpace::Type
    } else if has("service") {
        IdSpace::Service
    } else {
        IdSpace::Essence
    }
}

/// One integer slot of a list's records that may hold another record's ID.
#[derive(Debug, Clone)]
pub struct Site {
    pub list: usize,
    pub off: usize,
    /// Field path, e.g. "pages[2].id_goods[5]".
    pub path: String,
    /// Structs named by the field's `refs` (empty for by-ID sites).
    pub refs: Vec<String>,
    /// For by-ID sites: the space the field's name implies.
    pub space: Option<IdSpace>,
}

/// Arrays larger than this are not expanded into sites (they hold data, not links).
const MAX_ARRAY: usize = 512;

/// The integer leaves of a definition with absolute offsets and paths.
pub fn sites_of(list: usize, fields: &[Field], out: &mut Vec<Site>) {
    fn walk(list: usize, fields: &[Field], base: usize, prefix: &str, top: bool, out: &mut Vec<Site>) {
        for f in fields {
            let path = if prefix.is_empty() { f.name.clone() } else { format!("{prefix}.{}", f.name) };
            leaf(list, f, &f.t, base + f.off, path, top, out);
        }
    }
    fn leaf(list: usize, f: &Field, t: &Ty, off: usize, path: String, top: bool, out: &mut Vec<Site>) {
        match t {
            Ty::Array { n, stride, t } if *n <= MAX_ARRAY => {
                for i in 0..*n {
                    leaf(list, f, t, off + i * stride, format!("{path}[{i}]"), false, out);
                }
            }
            Ty::Struct { fields } => walk(list, fields, off, &path, false, out),
            Ty::I32 | Ty::U32 => {
                // The record's own ID (first field) and enum/path fields are not links.
                if (top && off == 0) || f.e.is_some() || f.display.is_some() {
                    return;
                }
                if !f.refs.is_empty() {
                    out.push(Site { list, off, path, refs: f.refs.clone(), space: None });
                } else if id_like(&f.name) {
                    out.push(Site { list, off, path, refs: Vec::new(), space: Some(field_space(&f.name)) });
                }
            }
            _ => {}
        }
    }
    walk(list, fields, 0, "", true, out);
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Referrer {
    pub list: usize,
    pub row: usize,
    pub id: u32,
    pub name: String,
    pub field: String,
    /// "declared" (the field's refs name this list) or "id" (matched by field name and ID space).
    pub how: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferencedBy {
    pub id: u32,
    pub referrers: Vec<Referrer>,
    /// More references exist than are listed.
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_fields_and_spaces_follow_names() {
        for name in ["id_goods", "id_to_make", "item_id", "id", "id2", "id_obj", "gem_config_id", "ids", "IconPathID", "Model_Path_ID"] {
            assert!(id_like(name), "{name}");
        }
        for name in ["level", "price", "num_goods", "probability", "width", "valid", "idle_time", "grid"] {
            assert!(!id_like(name), "{name}");
        }
        assert_eq!(field_space("id_goods"), IdSpace::Essence);
        assert_eq!(field_space("id_tasks"), IdSpace::Other);
        assert_eq!(field_space("id_recipe"), IdSpace::Recipe);
        assert_eq!(field_space("gem_config_id"), IdSpace::Config);
        assert_eq!(field_space("id_addon3"), IdSpace::Addon);
        assert_eq!(list_space("EQUIPMENT_ESSENCE"), IdSpace::Essence);
        assert_eq!(list_space("RECIPE_ESSENCE"), IdSpace::Recipe);
        assert_eq!(list_space("EQUIPMENT_ADDON"), IdSpace::Addon);
        assert_eq!(list_space("GEM_CONFIG"), IdSpace::Config);
        // Follow setup_hash_map, which is the path used after loading a file.
        // Some add_structure overloads disagree with it for later configs.
        assert_eq!(registry_space("COLLISION_RAID_AWARD_CONFIG"), IdSpace::Config);
        assert_eq!(registry_space("BUFF_AREA_CONFIG"), IdSpace::Config);
        assert_eq!(registry_space("WAR_ROLE_CONFIG"), IdSpace::Essence);
        assert_eq!(registry_space("ITEM_TRADE_CONFIG"), IdSpace::Essence);
    }
}
