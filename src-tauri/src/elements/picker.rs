//! Bounded, on-demand choices for integer fields that point at records or
//! client resources. Opening a record never builds these results.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::refs::{self, IdSpace};
use super::search;
use super::Document;
use crate::client::Resources;

const LIMIT: usize = 80;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub list: usize,
    pub row: usize,
    pub off: usize,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub page: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub value: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultPage {
    pub kind: String,
    pub title: String,
    pub scope: String,
    pub current: Option<u32>,
    pub entries: Vec<Entry>,
    /// Matches before the response limit is applied.
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug)]
pub(crate) enum Source {
    Records(Vec<usize>),
    Resource(String),
    Dialogs,
}

#[derive(Debug)]
pub(crate) struct Spec {
    pub source: Source,
    kind: String,
    title: String,
    scope: String,
    current: Option<u32>,
}

fn rank(query: &str, current: Option<u32>, id: u32, lower_name: &str) -> Option<u8> {
    let query = query.trim();
    if query.is_empty() {
        return Some(if current == Some(id) { 0 } else { 4 });
    }
    if query.parse::<u32>().ok() == Some(id) {
        return Some(0);
    }
    let needle = query.to_lowercase();
    if lower_name == needle {
        Some(1)
    } else if lower_name.starts_with(&needle) {
        Some(2)
    } else if lower_name.contains(&needle) {
        Some(3)
    } else {
        None
    }
}

impl Document {
    /// Resolves what the field at the request offset can choose from. This is
    /// intentionally cheap; searching happens only after the document lock is
    /// released when client resources are involved.
    pub(crate) fn picker_spec(&self, request: &Request) -> Result<Spec, String> {
        let block = self.file.lists.get(request.list).ok_or("No such list")?;
        let (_, def) = self.def(request.list).ok_or("This list has no field layout")?;
        let slot = search::slots(def, block.item_size)
            .into_iter()
            .find(|slot| slot.off == request.off)
            .ok_or("No editable field starts at this offset")?;
        let bytes = self.file.record(request.list, request.row).ok_or("No such record")?;
        let current = slot.int(bytes).and_then(|value| u32::try_from(value).ok());
        let field = slot.name().to_string();

        if let Some(role @ ("skill" | "buff" | "title" | "path" | "icon" | "image")) = slot.display.as_deref() {
            return Ok(Spec {
                source: Source::Resource(role.to_string()),
                kind: role.to_string(),
                title: format!("Choose {field}"),
                scope: match role {
                    "skill" => "Skills from configs.pck",
                    "buff" => "Buffs from configs.pck",
                    "title" => "Titles from interfaces.pck",
                    "path" | "icon" => "Paths from path.data",
                    "image" => "Standalone images found in client packages",
                    _ => unreachable!(),
                }
                .to_string(),
                current,
            });
        }

        if field.eq_ignore_ascii_case("id_dialog") {
            return Ok(Spec { source: Source::Dialogs, kind: "dialog".into(), title: format!("Choose {field}"), scope: "NPC dialogs in this file".into(), current });
        }

        let (targets, scope) = if !slot.refs.is_empty() {
            let targets: Vec<usize> = slot.refs.iter().filter_map(|name| self.by_struct.get(name)).flatten().copied().collect();
            let scope = slot.refs.join(", ");
            (targets, scope)
        } else if refs::id_like(&field) {
            let space = refs::field_space(&field);
            if space == IdSpace::Other {
                return Err(format!("{field} points outside elements.data"));
            }
            let targets = self
                .lists
                .iter()
                .enumerate()
                .filter_map(|(list, resolved)| {
                    let name = resolved.struct_name.as_deref()?;
                    let matches = match space {
                        IdSpace::Type | IdSpace::Service => refs::list_space(name) == space,
                        _ => refs::registry_space(name) == space,
                    };
                    matches.then_some(list)
                })
                .collect();
            (targets, format!("{} ID space", format!("{space:?}").to_lowercase()))
        } else {
            return Err(format!("{field} does not have a value picker"));
        };

        if targets.is_empty() {
            return Err(format!("No matching lists exist for {field}"));
        }
        Ok(Spec { source: Source::Records(targets), kind: "reference".into(), title: format!("Choose {field}"), scope, current })
    }

    /// Items or monsters (by `kind`) for editors of other files (dyn_tasks.data).
    pub(crate) fn essence_spec(&self, kind: &str, current: Option<u32>) -> Result<Spec, String> {
        const NOT_ITEMS: [&str; 3] = ["MONSTER", "NPC", "MINE"];
        let targets: Vec<usize> = self.lists.iter().enumerate().filter_map(|(list, resolved)| {
            let name = resolved.struct_name.as_deref()?.to_ascii_uppercase();
            let wanted = match kind {
                "monster" => name == "MONSTER_ESSENCE",
                "npc" => name == "NPC_ESSENCE" || name == "MONSTER_ESSENCE",
                _ => refs::list_space(&name) == IdSpace::Essence && refs::registry_space(&name) == IdSpace::Essence && !NOT_ITEMS.iter().any(|prefix| name.starts_with(prefix)) && !name.ends_with("_CONFIG"),
            };
            wanted.then_some(list)
        }).collect();
        if targets.is_empty() {
            return Err(format!("This elements.data has no {kind} lists"));
        }
        let (title, scope) = match kind {
            "monster" => ("Choose a monster", "MONSTER_ESSENCE"),
            "npc" => ("Choose an NPC or monster", "NPC_ESSENCE and MONSTER_ESSENCE"),
            _ => ("Choose an item", "Item lists of elements.data"),
        };
        Ok(Spec { source: Source::Records(targets), kind: "reference".into(), title: title.into(), scope: scope.into(), current })
    }

    pub(crate) fn picker_records(&self, spec: &Spec, query: &str, page: usize) -> Result<ResultPage, String> {
        let Source::Records(targets) = &spec.source else { return Err("Picker source is not a record list".into()) };
        let targets: HashSet<usize> = targets.iter().copied().collect();
        let mut ranked: Vec<(u8, &super::FindEntry)> = self
            .find_entries()
            .iter()
            .filter(|entry| targets.contains(&entry.list))
            .filter_map(|entry| rank(query, spec.current, entry.id, &entry.lower).map(|rank| (rank, entry)))
            .collect();
        ranked.sort_by_key(|(rank, entry)| (*rank, entry.list, entry.index));
        let total = ranked.len();
        let entries = ranked
            .into_iter()
            .skip(page.saturating_mul(LIMIT))
            .take(LIMIT)
            .map(|(_, entry)| {
                let bytes = self.file.record(entry.list, entry.index).unwrap_or_default();
                let icon = self.record_icon(bytes, Self::icon_field(self.def(entry.list).map(|(_, def)| def)));
                Entry {
                    value: entry.id,
                    name: if entry.name.is_empty() { "Unnamed record".into() } else { entry.name.clone() },
                    detail: Some(self.list_name(entry.list)),
                    description: None,
                    icon,
                    list: Some(entry.list),
                    row: Some(entry.index),
                }
            })
            .collect();
        Ok(spec.page(entries, total, page))
    }

    pub(crate) fn picker_dialogs(&self, spec: &Spec, query: &str, page: usize) -> Result<ResultPage, String> {
        if !matches!(&spec.source, Source::Dialogs) {
            return Err("Picker source is not NPC dialogs".into());
        }
        let talks = self.talks()?;
        let mut ranked: Vec<(u8, super::TalkSummary)> = talks
            .into_iter()
            .filter_map(|talk| {
                let lower = talk.title.to_lowercase();
                rank(query, spec.current, talk.id, &lower).map(|rank| (rank, talk))
            })
            .collect();
        ranked.sort_by_key(|(rank, talk)| (*rank, talk.index));
        let total = ranked.len();
        let entries = ranked
            .into_iter()
            .skip(page.saturating_mul(LIMIT))
            .take(LIMIT)
            .map(|(_, talk)| Entry {
                value: talk.id,
                name: if talk.title.is_empty() { "Untitled dialog".into() } else { talk.title },
                detail: Some(format!("{} windows · {} options", talk.windows, talk.options)),
                description: talk.used_by.map(|name| format!("Used by {name}")),
                icon: None,
                list: None,
                row: Some(talk.index),
            })
            .collect();
        Ok(spec.page(entries, total, page))
    }
}

impl Spec {
    fn page(&self, entries: Vec<Entry>, total: usize, page: usize) -> ResultPage {
        ResultPage { kind: self.kind.clone(), title: self.title.clone(), scope: self.scope.clone(), current: self.current, entries, total, page, page_size: LIMIT }
    }
}

/// Searches client package resources after the document lock has been
/// released. Loading and parsing the selected table remains lazy.
pub(crate) fn resource_page(resources: &Resources, spec: &Spec, query: &str, page: usize) -> Result<ResultPage, String> {
    let Source::Resource(role) = &spec.source else { return Err("Picker source is not a client resource".into()) };
    let (choices, total) = resources.search_choices(role, query, spec.current, page.saturating_mul(LIMIT), LIMIT)?;
    let entries = choices
        .into_iter()
        .map(|choice| Entry {
            value: choice.id,
            name: choice.name,
            detail: None,
            description: choice.description,
            icon: choice.icon.then_some(choice.id),
            list: None,
            row: None,
        })
        .collect();
    Ok(spec.page(entries, total, page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_prefers_the_current_value_then_names() {
        assert_eq!(rank("", Some(42), 42, "answer"), Some(0));
        assert_eq!(rank("", Some(42), 7, "answer"), Some(4));
        assert_eq!(rank("42", None, 42, "other"), Some(0));
        assert_eq!(rank("answer", None, 7, "answer"), Some(1));
        assert_eq!(rank("ans", None, 7, "answer"), Some(2));
        assert_eq!(rank("swe", None, 7, "answer"), Some(3));
        assert_eq!(rank("missing", None, 7, "answer"), None);
    }

    #[test]
    fn items_and_monsters_for_other_editors() {
        let path = String::from("E:/Games/ForsakenJD/element/data/elements.data");
        if !std::path::Path::new(&path).is_file() { return; }
        let doc = Document::open(path, std::sync::Arc::new(crate::elements::format::Catalog::load(None))).unwrap();
        let monsters = doc.essence_spec("monster", None).unwrap();
        let page = doc.picker_records(&monsters, "", 0).unwrap();
        assert!(page.total > 1000 && page.entries.iter().all(|entry| entry.detail.as_deref().is_some_and(|list| list.contains("MONSTER"))));
        let items = doc.essence_spec("item", None).unwrap();
        let page = doc.picker_records(&items, "", 0).unwrap();
        assert!(page.total > 10_000);
        assert!(page.entries.iter().all(|entry| !entry.detail.as_deref().unwrap_or("").contains("MONSTER")));
    }
}
