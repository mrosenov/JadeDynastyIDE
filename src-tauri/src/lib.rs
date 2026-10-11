mod aipolicy;
mod backup;
mod client;
mod domain;
mod dyn_objects;
mod extra_drops;
mod dyn_tasks;
mod elements;
mod gshop;
mod path_data;
mod quiz;
mod map_regions;
mod npcgen;
mod settings;
mod task_npc;
mod vipaward;
pub mod tasks;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use client::{ClientInfo, Resources};
use settings::Settings;

use elements::format::{
    builtin_set, delete_set, delete_user_list, delete_user_set, save_user_list, save_user_set, Catalog, ListDef, NamedSet, SetKind,
    SetOrigin,
};
use elements::{Document, FileSummary, ImportCandidate, ListSchema, RecordDetail, RecordRow};
use serde::Serialize;
use tauri::{Manager, State};

struct AppState {
    document: Mutex<Option<Document>>,
    /// The separately opened static task set. Root records are decoded lazily.
    tasks: Mutex<Option<tasks::browser::TaskDocument>>,
    /// A second file compared with the open one (locked after `document`).
    compared: Mutex<Option<Document>>,
    /// The task set compared with the open one (lock order: tasks, then compared_tasks).
    compared_tasks: Mutex<Option<tasks::compare::ComparedTasks>>,
    /// The translated task set and its previewed plan (lock order: tasks, then task_translation).
    task_translation: Mutex<Option<tasks::translate::TranslationSource>>,
    /// Cancels and reports the running advanced task search.
    task_search: Arc<tasks::search::SearchControl>,
    /// The open dyn_tasks.data (locked after `tasks` and `document`, never while holding them).
    dyn_tasks: Mutex<Option<dyn_tasks::DynDocument>>,
    /// Another dyn_tasks.data opened for comparing (locked after `dyn_tasks`).
    compared_dyn: Mutex<Option<dyn_tasks::ComparedPack>>,
    /// The open npcgen.data (one server map).
    npcgen: Mutex<Option<npcgen::Document>>,
    /// The open gshop shop (client and server copy together).
    gshop: Mutex<Option<gshop::Document>>,
    /// Another shop (file or JSON export) opened read-only for comparing (lock order: gshop, then compared_gshop).
    compared_gshop: Mutex<Option<gshop::ComparedShop>>,
    /// The open aipolicy.data (read only for now).
    aipolicy: Mutex<Option<aipolicy::Document>>,
    /// The extra_drops.sev opened last, and its index (read from the saved file, refreshed when it changes).
    drops_source: Mutex<Option<(std::path::PathBuf, Option<std::time::SystemTime>, Arc<extra_drops::DropIndex>)>>,
    /// Another aipolicy.data or JSON export, read-only (lock order: aipolicy, then compared_aipolicy).
    compared_aipolicy: Mutex<Option<aipolicy::compare::Compared>>,
    /// Another npcgen.data opened read-only for comparing (lock order: npcgen, then compared_npcgen).
    compared_npcgen: Mutex<Option<npcgen::ComparedGen>>,
    catalog: RwLock<Arc<Catalog>>,
    /// The user's data folder: `layouts/`, `enums/` and `masks/` written by the editors.
    user_dir: PathBuf,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// The configured game client's paths and icons.
    resources: RwLock<Option<Arc<Resources>>>,
    /// Changes whenever the client changes, so icon URLs are not served stale.
    icon_generation: AtomicU32,
}

impl AppState {
    fn resources(&self) -> Option<Arc<Resources>> {
        self.resources.read().ok().and_then(|r| r.clone())
    }

    /// Loads the client named in the settings, for icons and paths. The
    /// open document starts using it right away.
    fn apply_client(&self, settings: &Settings) -> (Option<ClientInfo>, Option<String>) {
        let (resources, info, error) = match settings.client_dir().map(|d| client::inspect(std::path::Path::new(d))) {
            Some(Ok(info)) => (Some(Arc::new(Resources::new(info.clone()))), Some(info), None),
            Some(Err(e)) => (None, None, Some(e)),
            None => (None, None, None),
        };
        if let Some(res) = resources.clone() {
            // Index the icon atlas and path table in the background.
            std::thread::spawn(move || {
                if let Err(e) = res.paths() {
                    eprintln!("client paths: {e}");
                }
                if let Err(e) = res.item_icons() {
                    eprintln!("client icons: {e}");
                }
            });
        }
        if let Ok(mut r) = self.resources.write() {
            *r = resources.clone();
        }
        self.icon_generation.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut doc) = self.document.lock() {
            if let Some(doc) = doc.as_mut() {
                doc.resources = resources;
            }
        }
        (info, error)
    }

    fn settings_view(&self, client: Option<ClientInfo>, client_error: Option<String>) -> SettingsView {
        SettingsView {
            settings: self.settings.lock().map(|s| s.clone()).unwrap_or_default(),
            client,
            client_error,
            icon_generation: self.icon_generation.load(Ordering::Relaxed),
        }
    }
    fn catalog(&self) -> Arc<Catalog> {
        self.catalog.read().map(|c| c.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    fn with_document_mut<T>(&self, f: impl FnOnce(&mut Document) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self.document.lock().map_err(|_| "State lock poisoned")?;
        f(guard.as_mut().ok_or("No file is open")?)
    }

    fn with_document<T>(&self, f: impl FnOnce(&Document) -> Result<T, String>) -> Result<T, String> {
        let guard = self.document.lock().map_err(|_| "State lock poisoned")?;
        f(guard.as_ref().ok_or("No file is open")?)
    }

    /// Changes the user's schema files for the open document, then reloads
    /// the catalog and re-reads the document with it.
    fn change_schemas(&self, change: impl FnOnce(&Document) -> Result<(), String>) -> Result<FileSummary, String> {
        {
            let guard = self.document.lock().map_err(|_| "State lock poisoned")?;
            change(guard.as_ref().ok_or("No file is open")?)?;
        }
        self.reload_catalog()?.ok_or_else(|| "No file is open".into())
    }

    /// Reloads the catalog from disk; an open document is re-read with it.
    fn reload_catalog(&self) -> Result<Option<FileSummary>, String> {
        let mut guard = self.document.lock().map_err(|_| "State lock poisoned")?;
        let catalog = Arc::new(Catalog::load(Some(&self.user_dir)));
        *self.catalog.write().map_err(|_| "State lock poisoned")? = catalog.clone();
        let Some(doc) = guard.as_ref() else { return Ok(None) };
        let reloaded = doc.reload(catalog)?;
        let summary = reloaded.summary();
        *guard = Some(reloaded);
        Ok(Some(summary))
    }
}

#[tauri::command]
async fn open_elements(path: String, state: State<'_, AppState>) -> Result<FileSummary, String> {
    let catalog = state.catalog();
    let mut doc = tauri::async_runtime::spawn_blocking(move || Document::open(path, catalog))
        .await
        .map_err(|e| e.to_string())??;
    doc.resources = state.resources();
    let summary = doc.summary();
    *state.document.lock().map_err(|_| "State lock poisoned")? = Some(doc);
    Ok(summary)
}

#[tauri::command]
async fn open_npcgen(path: String, state: State<'_, AppState>) -> Result<npcgen::View, String> {
    let document = tauri::async_runtime::spawn_blocking(move || npcgen::Document::open(path)).await.map_err(|error| error.to_string())??;
    let view = document.view();
    *state.npcgen.lock().map_err(|_| "State lock poisoned")? = Some(document);
    Ok(view)
}

#[tauri::command]
fn npcgen_view(state: State<'_, AppState>) -> Result<Option<npcgen::View>, String> {
    Ok(state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_ref().map(npcgen::Document::view))
}

#[tauri::command]
fn npcgen_item(section: npcgen::Section, index: usize, state: State<'_, AppState>) -> Result<npcgen::Item, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open npcgen.data first")?.item(section, index)
}

#[tauri::command]
fn set_npcgen_item(index: usize, item: npcgen::Item, label: String, state: State<'_, AppState>) -> Result<npcgen::View, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.set_item(index, item, &label)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NpcGenCloneResult {
    view: npcgen::View,
    index: usize,
}

#[tauri::command]
fn clone_npcgen_item(section: npcgen::Section, index: usize, state: State<'_, AppState>) -> Result<NpcGenCloneResult, String> {
    let (view, index) = state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.clone_item(section, index)?;
    Ok(NpcGenCloneResult { view, index })
}

#[tauri::command]
fn delete_npcgen_item(section: npcgen::Section, index: usize, state: State<'_, AppState>) -> Result<npcgen::View, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.delete_item(section, index)
}

#[tauri::command]
fn undo_npcgen(state: State<'_, AppState>) -> Result<npcgen::View, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.undo()
}

#[tauri::command]
fn redo_npcgen(state: State<'_, AppState>) -> Result<npcgen::View, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.redo()
}

#[tauri::command]
async fn save_npcgen(target: Option<String>, backup: bool, replace_changed: bool, state: State<'_, AppState>) -> Result<npcgen::SaveReport, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.save(target.as_deref(), backup, replace_changed)
}

#[tauri::command]
async fn open_task_npc(path: String) -> Result<task_npc::FileView, String> {
    tauri::async_runtime::spawn_blocking(move || task_npc::open(path)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_task_npc(request: task_npc::SaveRequest) -> Result<task_npc::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || task_npc::save(request)).await.map_err(|error| error.to_string())?
}

/// Map names of the configured client (configs.pck instance.txt), or none without a client.
#[tauri::command]
async fn client_map_names(state: State<'_, AppState>) -> Result<Vec<(i32, String)>, String> {
    let Some(resources) = state.resources() else { return Ok(Vec::new()) };
    tauri::async_runtime::spawn_blocking(move || resources.instances().map(|maps| maps.iter().map(|map| (map.id, map.name.clone())).collect()))
        .await
        .map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClientMap {
    #[serde(flatten)]
    instance: client::instances::Instance,
    /// `Surfaces/MidMaps/<path>.dds` exists.
    has_image: bool,
}

/// Running game clients (elementclient.exe), for taking the character's position.
#[tauri::command]
async fn game_clients() -> Result<Vec<client::game::RunningClient>, String> {
    tauri::async_runtime::spawn_blocking(client::game::running_clients).await.map_err(|error| error.to_string())
}

/// The character's position in a running game client (read-only; the exe is scanned once for where it keeps it).
#[tauri::command]
async fn game_position(pid: u32) -> Result<client::game::Position, String> {
    tauri::async_runtime::spawn_blocking(move || client::game::read_position(pid)).await.map_err(|error| error.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NearbyRow {
    #[serde(flatten)]
    entity: client::nearby::Entity,
    /// "npc", "monster", "mine", "dynamic", "item" (dropped items, money and other matters) or "unknown"
    /// (an NPC or monster without an open elements.data that names it).
    class: &'static str,
    label: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NearbyFetch {
    player: client::game::Position,
    rows: Vec<NearbyRow>,
}

/// Everything a running game client has loaded around the character (read-only), named and classified
/// with the open elements.data when there is one.
#[tauri::command]
async fn game_nearby(pid: u32, state: State<'_, AppState>) -> Result<NearbyFetch, String> {
    let (player, entities) = tauri::async_runtime::spawn_blocking(move || {
        client::game::with_client(pid, |path, chain, base, read| {
            let run = client::game::game_run(chain, base, &read)?;
            let player = client::game::follow(chain, base, read)?;
            Ok((player, client::nearby::fetch(path, read, run)?))
        })
    })
    .await
    .map_err(|error| error.to_string())??;
    let document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let rows = entities
        .into_iter()
        .map(|entity| {
            use client::nearby::EntityKind;
            let found = if entity.kind == EntityKind::Dynamic { None } else { document.as_ref().and_then(|document| document.essence_struct(entity.template)) };
            let class = match (entity.kind, found.as_ref().map(|(name, _)| name.as_str())) {
                (EntityKind::Dynamic, _) => "dynamic",
                (EntityKind::Npc, Some("MONSTER_ESSENCE")) => "monster",
                (EntityKind::Npc, Some(_)) => "npc",
                (EntityKind::Npc, None) => "unknown",
                (EntityKind::Matter, _) if entity.dropper != 0 => "item",
                (EntityKind::Matter, Some("MINE_ESSENCE")) => "mine",
                (EntityKind::Matter, Some(_)) => "item",
                (EntityKind::Matter, None) if document.is_some() => "item",
                (EntityKind::Matter, None) => "mine",
            };
            NearbyRow { entity, class, label: found.map(|(_, label)| label) }
        })
        .collect();
    Ok(NearbyFetch { player, rows })
}

// ---------------------------------------------------------------- gshop.data

fn with_gshop<T>(state: &State<'_, AppState>, work: impl FnOnce(&mut gshop::Document) -> Result<T, String>) -> Result<T, String> {
    work(state.gshop.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open a gshop file first")?)
}

/// Opens a shop file (gshop.data, gshop1.data or gshop2.data) with the preferred layout when it fits,
/// else the first that does (user layouts first). `NO_LAYOUT:{…}` when none fits.
#[tauri::command]
async fn open_gshop(path: String, layout: Option<String>, state: State<'_, AppState>) -> Result<gshop::View, String> {
    let layouts = gshop::layout::all(Some(&state.user_dir));
    let document = tauri::async_runtime::spawn_blocking(move || gshop::Document::open(&path, &layouts, layout.as_deref())).await.map_err(|error| error.to_string())??;
    let view = document.view();
    *state.gshop.lock().map_err(|_| "State lock poisoned")? = Some(document);
    Ok(view)
}

#[tauri::command]
fn gshop_view(state: State<'_, AppState>) -> Result<Option<gshop::View>, String> {
    Ok(state.gshop.lock().map_err(|_| "State lock poisoned")?.as_ref().map(gshop::Document::view))
}

#[tauri::command]
fn gshop_item(index: usize, state: State<'_, AppState>) -> Result<gshop::ShopItem, String> {
    with_gshop(&state, |document| document.item(index))
}

#[tauri::command]
fn set_gshop_item(index: usize, item: gshop::ShopItem, label: String, state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.set_item(index, item, &label))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GShopCloneResult {
    view: gshop::View,
    index: usize,
}

#[tauri::command]
fn clone_gshop_item(index: usize, state: State<'_, AppState>) -> Result<GShopCloneResult, String> {
    with_gshop(&state, |document| document.clone_item(index).map(|(view, index)| GShopCloneResult { view, index }))
}

#[tauri::command]
fn delete_gshop_item(index: usize, state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.delete_item(index))
}

#[tauri::command]
fn move_gshop_item(index: usize, to: usize, state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.move_item(index, to))
}

#[tauri::command]
fn edit_gshop_categories(op: gshop::CategoryOp, state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.edit_categories(op))
}

#[tauri::command]
fn undo_gshop(state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.undo())
}

#[tauri::command]
fn redo_gshop(state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.redo())
}

#[tauri::command]
fn save_gshop(backup: bool, replace_changed: bool, state: State<'_, AppState>) -> Result<gshop::SaveReport, String> {
    with_gshop(&state, |document| document.save(backup, replace_changed))
}

/// Compares the open shop with another shop file or a JSON export (`path`), or again with the one chosen before.
#[tauri::command]
async fn compare_gshop(path: Option<String>, state: State<'_, AppState>) -> Result<gshop::ShopComparison, String> {
    if let Some(path) = path {
        let layouts = gshop::layout::all(Some(&state.user_dir));
        let other = tauri::async_runtime::spawn_blocking(move || gshop::ComparedShop::open(path, &layouts)).await.map_err(|error| error.to_string())??;
        *state.compared_gshop.lock().map_err(|_| "State lock poisoned")? = Some(other);
    }
    let document = state.gshop.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_gshop.lock().map_err(|_| "State lock poisoned")?;
    Ok(document.as_ref().ok_or("Open a gshop file first")?.compare(compared.as_ref().ok_or("Choose a shop to compare with")?))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GShopCopyResult {
    view: gshop::View,
    report: gshop::CopyReport,
}

/// Copies the picked items of the compared shop (by their position there) into the open one.
#[tauri::command]
fn copy_gshop(picks: Vec<usize>, state: State<'_, AppState>) -> Result<GShopCopyResult, String> {
    let mut document = state.gshop.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_gshop.lock().map_err(|_| "State lock poisoned")?;
    let (view, report) = document.as_mut().ok_or("Open a gshop file first")?.copy_compared(compared.as_ref().ok_or("Choose a shop to compare with")?, &picks)?;
    Ok(GShopCopyResult { view, report })
}

#[tauri::command]
fn close_gshop_comparison(state: State<'_, AppState>) -> Result<(), String> {
    *state.compared_gshop.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

/// Writes a JSON export of the open shop (all items, or the picked positions).
#[tauri::command]
fn export_gshop_json(target: String, picks: Option<Vec<usize>>, state: State<'_, AppState>) -> Result<gshop::ExportCounts, String> {
    let (json, counts) = with_gshop(&state, |document| document.export_json(picks.as_deref()))?;
    std::fs::write(&target, json).map_err(|error| format!("Could not write {target}: {error}"))?;
    Ok(counts)
}

/// The shop items (at `picks`, or all) whose name differs from elements.data or whose description differs
/// from the client's item_ext_desc.txt. Locks one thing at a time: the shop, the source, the shop.
#[tauri::command]
async fn gshop_text_updates(field: gshop::TextField, picks: Option<Vec<usize>>, state: State<'_, AppState>) -> Result<gshop::TextPreview, String> {
    let mut ids = with_gshop(&state, |document| Ok(document.item_ids(picks.as_deref())))?;
    ids.sort_unstable();
    ids.dedup();
    let texts: HashMap<u32, String> = match field {
        gshop::TextField::Name => {
            let document = state.document.lock().map_err(|_| "State lock poisoned")?;
            let document = document.as_ref().ok_or("Open the elements.data that matches this shop first")?;
            ids.iter().filter_map(|&id| document.essence_name(id).map(|name| (id, name))).collect()
        }
        gshop::TextField::Description => {
            let resources = state.resources().ok_or("Set the game client folder in Settings first: descriptions come from its configs.pck")?;
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = resources.table(client::table::ITEM_DESC).as_ref() {
                    return Err(error.clone());
                }
                Ok(ids.iter().filter_map(|&id| resources.text(client::table::ITEM_DESC, id).map(|text| (id, gshop::texts::shop_text(&text)))).collect())
            })
            .await
            .map_err(|error| error.to_string())??
        }
    };
    with_gshop(&state, |document| document.text_updates(field, picks.as_deref(), &texts))
}

/// Writes the picked name or description updates as one undo step.
#[tauri::command]
fn apply_gshop_texts(field: gshop::TextField, rows: Vec<gshop::TextApply>, state: State<'_, AppState>) -> Result<gshop::View, String> {
    with_gshop(&state, |document| document.apply_texts(field, &rows))
}

/// Proposes a layout for a shop file (`path`, or the open shop's file) by finding where it keeps the
/// fields of `reference`, a shop file the editor reads.
#[tauri::command]
async fn propose_gshop_layout(reference: String, path: Option<String>, state: State<'_, AppState>) -> Result<gshop::align::LayoutProposal, String> {
    let path = match path {
        Some(path) => path,
        None => with_gshop(&state, |document| Ok(document.path().display().to_string()))?,
    };
    let layouts = gshop::layout::all(Some(&state.user_dir));
    tauri::async_runtime::spawn_blocking(move || {
        let known = std::fs::read(&reference).map_err(|error| format!("Could not read {reference}: {error}"))?;
        let target = std::fs::read(&path).map_err(|error| format!("Could not read {path}: {error}"))?;
        gshop::align::propose(&known, &layouts, &target)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn open_vipaward(path: String) -> Result<vipaward::FileView, String> {
    tauri::async_runtime::spawn_blocking(move || vipaward::open(path)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_vipaward(request: vipaward::SaveRequest) -> Result<vipaward::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || vipaward::save(request)).await.map_err(|error| error.to_string())?
}

/// Problems by the server's rules; items are checked against the open elements.data (existence, stack limit).
#[tauri::command]
fn vipaward_problems(awards: Vec<vipaward::Award>, record_size: usize, state: State<'_, AppState>) -> Result<Vec<vipaward::Problem>, String> {
    let items = {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        document.as_ref().map(|document| {
            vipaward::item_ids(&awards)
                .into_iter()
                .map(|id| (id, vipaward::ItemInfo { exists: document.resolve_essence_id(id).is_some(), stack: document.essence_number(id, "pile_num_max").and_then(|value| u32::try_from(value).ok()) }))
                .collect::<HashMap<_, _>>()
        })
    };
    Ok(vipaward::problems(&awards, record_size, items.as_ref()))
}

/// The client's names for the award levels (its VIP window's buttons).
#[tauri::command]
async fn vipaward_level_names(state: State<'_, AppState>) -> Result<vipaward::LevelNames, String> {
    let resources = state.resources();
    tauri::async_runtime::spawn_blocking(move || Ok(vipaward::level_names(resources.as_ref().and_then(|resources| resources.ingame_strings().ok()))))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn open_aipolicy(path: String, state: State<'_, AppState>) -> Result<aipolicy::FileView, String> {
    let document = tauri::async_runtime::spawn_blocking(move || aipolicy::Document::open(&path)).await.map_err(|error| error.to_string())??;
    let view = document.view();
    *state.aipolicy.lock().map_err(|_| "State lock poisoned")? = Some(document);
    Ok(view)
}

fn with_aipolicy<T>(state: &State<'_, AppState>, work: impl FnOnce(&aipolicy::Document) -> Result<T, String>) -> Result<T, String> {
    work(state.aipolicy.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open aipolicy.data first")?)
}

fn with_aipolicy_mut<T>(state: &State<'_, AppState>, work: impl FnOnce(&mut aipolicy::Document) -> Result<T, String>) -> Result<T, String> {
    work(state.aipolicy.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open aipolicy.data first")?)
}

/// Replaces a trigger with an edit (checked: shapes the server cannot load are refused).
#[tauri::command]
fn aipolicy_set_trigger(policy: usize, trigger: usize, edit: aipolicy::TriggerEdit, state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, |document| document.set_trigger(policy, trigger, &edit))
}

#[tauri::command]
fn aipolicy_trigger_action(policy: usize, trigger: usize, action: aipolicy::ListAction, state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, |document| document.trigger_action(policy, trigger, &action))
}

#[tauri::command]
fn aipolicy_policy_action(policy: usize, action: aipolicy::ListAction, state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, |document| document.policy_action(policy, &action))
}

/// Sub-triggers nothing runs (one policy or all): (policy position, policy ID, trigger IDs).
#[tauri::command]
fn aipolicy_unused_subtriggers(policy: Option<usize>, state: State<'_, AppState>) -> Result<Vec<(usize, u32, Vec<u32>)>, String> {
    with_aipolicy(&state, |document| Ok(document.unused_subtriggers(policy).into_iter().map(|(index, ids)| (index, document.file.policies[index].id, ids)).collect()))
}

#[tauri::command]
fn aipolicy_cleanup(policy: Option<usize>, shown: Option<usize>, state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, |document| document.cleanup(policy, shown))
}

/// Every AI edit, newest first, with what each changed and whether it can be reverted on its own.
#[tauri::command]
fn aipolicy_history(state: State<'_, AppState>) -> Result<Vec<aipolicy::HistoryRow>, String> {
    with_aipolicy(&state, |document| Ok(document.history()))
}

#[tauri::command]
fn aipolicy_revert_entry(id: u64, shown: Option<usize>, state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, |document| document.revert_entry(id, shown))
}

#[tauri::command]
fn aipolicy_revert_all(state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, aipolicy::Document::revert_all)
}

#[tauri::command]
fn aipolicy_undo(state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, aipolicy::Document::undo)
}

#[tauri::command]
fn aipolicy_redo(state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, aipolicy::Document::redo)
}

#[tauri::command]
async fn aipolicy_save(target: Option<String>, backup: bool, replace_changed: bool, state: State<'_, AppState>) -> Result<aipolicy::SaveReport, String> {
    with_aipolicy_mut(&state, |document| document.save(target.as_deref(), backup, replace_changed))
}

/// Problems by the server's rules; skills, monsters, items, mines and tasks are checked against the client and the
/// open elements.data and tasks.data. One lock at a time: the policies, the other files, the policies again.
#[tauri::command]
async fn aipolicy_problems(state: State<'_, AppState>) -> Result<Vec<aipolicy::Problem>, String> {
    let (skills, elements, tasks) = with_aipolicy(&state, |document| Ok(document.referenced_ids()))?;
    let mut context = aipolicy::ProblemContext::default();
    if let Some(resources) = state.resources() {
        context.skills = Some(tauri::async_runtime::spawn_blocking(move || skills.into_iter().filter(|&id| resources.skill_name(id).is_some()).collect()).await.map_err(|error| error.to_string())?);
    }
    {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        if let Some(document) = document.as_ref() {
            context.elements = Some(elements.into_iter().filter(|&id| document.resolve_essence_id(id).is_some()).collect());
            context.monsters = document.monsters_by_policy();
        }
    }
    if let Some(names) = quest_names(&state) {
        context.tasks = Some(tasks.into_iter().filter(|id| names.contains_key(id)).collect());
    }
    with_aipolicy(&state, |document| Ok(document.problems(&context)))
}

/// The condition, operation and target types the open file's server reads, with their fields.
#[tauri::command]
fn aipolicy_catalog(state: State<'_, AppState>) -> Result<aipolicy::TypeCatalog, String> {
    with_aipolicy(&state, |document| Ok(aipolicy::catalog(aipolicy::is_newer(&document.file))))
}

/// Skills by name or ID (client skillstr.txt), for the AI editor's pickers.
#[tauri::command]
async fn pick_skill(query: String, page: usize, current: Option<u32>, state: State<'_, AppState>) -> Result<elements::picker::ResultPage, String> {
    let resources = state.resources().ok_or("Set the game client folder in Settings to search skills")?;
    tauri::async_runtime::spawn_blocking(move || elements::picker::resource_page(&resources, &elements::picker::Spec::resource("skill", current), &query, page)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn aipolicy_view(state: State<'_, AppState>) -> Result<Option<aipolicy::FileView>, String> {
    Ok(state.aipolicy.lock().map_err(|_| "State lock poisoned")?.as_ref().map(aipolicy::Document::view))
}

#[tauri::command]
fn aipolicy_policy(index: usize, state: State<'_, AppState>) -> Result<aipolicy::PolicyView, String> {
    with_aipolicy(&state, |document| document.policy(index))
}

/// Monsters of the open elements.data by the policy they use (empty without one).
fn monsters_by_policy(state: &State<'_, AppState>) -> HashMap<u32, Vec<(u32, String)>> {
    state.document.lock().ok().and_then(|document| document.as_ref().map(elements::Document::monsters_by_policy)).unwrap_or_default()
}

/// Compares the open aipolicy.data with another one or a JSON export (`path`), or again with the one chosen before.
#[tauri::command]
async fn compare_aipolicy(path: Option<String>, state: State<'_, AppState>) -> Result<aipolicy::compare::CompareSummary, String> {
    if let Some(path) = path {
        let other = tauri::async_runtime::spawn_blocking(move || aipolicy::compare::Compared::open(&path)).await.map_err(|error| error.to_string())??;
        *state.compared_aipolicy.lock().map_err(|_| "State lock poisoned")? = Some(other);
    }
    let document = state.aipolicy.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_aipolicy.lock().map_err(|_| "State lock poisoned")?;
    Ok(aipolicy::compare::compare(&document.as_ref().ok_or("Open aipolicy.data first")?.file, compared.as_ref().ok_or("Choose a file to compare with")?))
}

/// One policy (by ID) in both files, with its triggers paired.
#[tauri::command]
fn aipolicy_compare_policy(id: u32, state: State<'_, AppState>) -> Result<aipolicy::compare::PolicyDiff, String> {
    let document = state.aipolicy.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_aipolicy.lock().map_err(|_| "State lock poisoned")?;
    aipolicy::compare::policy_diff(&document.as_ref().ok_or("Open aipolicy.data first")?.file, compared.as_ref().ok_or("Choose a file to compare with")?, id)
}

/// Copies whole policies or some of their triggers from the compared file (one undo step).
#[tauri::command]
fn copy_aipolicy(items: Vec<aipolicy::compare::CopyItem>, shown: Option<usize>, state: State<'_, AppState>) -> Result<aipolicy::compare::CopyResult, String> {
    let mut document = state.aipolicy.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_aipolicy.lock().map_err(|_| "State lock poisoned")?;
    document.as_mut().ok_or("Open aipolicy.data first")?.copy_compared(compared.as_ref().ok_or("Choose a file to compare with")?, &items, shown)
}

#[tauri::command]
fn close_aipolicy_comparison(state: State<'_, AppState>) -> Result<(), String> {
    *state.compared_aipolicy.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

/// Writes the policies at `policies` (all when none) as `jdide-aipolicy` JSON; returns how many.
#[tauri::command]
fn export_aipolicy_json(target: String, policies: Option<Vec<usize>>, state: State<'_, AppState>) -> Result<usize, String> {
    let (json, count) = with_aipolicy(&state, |document| Ok((aipolicy::compare::to_json(&document.file, policies.as_deref(), &document.path)?, policies.as_ref().map_or(document.file.policies.len(), Vec::len))))?;
    std::fs::write(&target, json).map_err(|error| format!("Could not write {target}: {error}"))?;
    Ok(count)
}

/// Texts (trigger names, talk texts, summon names) another aipolicy.data or JSON export has differently.
#[tauri::command]
async fn aipolicy_translation(path: String, state: State<'_, AppState>) -> Result<aipolicy::compare::TranslationPreview, String> {
    let source = tauri::async_runtime::spawn_blocking(move || aipolicy::compare::Compared::open(&path)).await.map_err(|error| error.to_string())??;
    with_aipolicy(&state, |document| Ok(document.translation(&source.file)))
}

#[tauri::command]
fn apply_aipolicy_texts(updates: Vec<aipolicy::compare::TextUpdate>, shown: Option<usize>, state: State<'_, AppState>) -> Result<aipolicy::EditResult, String> {
    with_aipolicy_mut(&state, |document| document.apply_texts(&updates, shown))
}

/// Where the records of these Essence IDs store their AI policy (open elements.data).
#[tauri::command]
fn monster_policy_slots(ids: Vec<u32>, state: State<'_, AppState>) -> Result<Vec<elements::PolicySlot>, String> {
    let document = state.document.lock().map_err(|_| "State lock poisoned")?;
    Ok(document.as_ref().ok_or("Open elements.data first")?.policy_slots(&ids))
}

/// Sets the AI policy of records in the open elements.data (one undo step there).
#[tauri::command]
fn set_monster_policies(places: Vec<(usize, usize, usize, u32)>, policy: u32, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    let mut document = state.document.lock().map_err(|_| "State lock poisoned")?;
    document.as_mut().ok_or("Open elements.data first")?.set_policy_places(&places, policy)
}

#[tauri::command]
fn aipolicy_used_by(state: State<'_, AppState>) -> Result<HashMap<u32, Vec<(u32, String)>>, String> {
    Ok(monsters_by_policy(&state))
}

/// Policies matching a search (IDs, the monsters that use them, names of triggers, talk texts, IDs operations name).
#[tauri::command]
fn aipolicy_search(query: String, state: State<'_, AppState>) -> Result<Vec<usize>, String> {
    let monsters = monsters_by_policy(&state);
    with_aipolicy(&state, |document| Ok(document.search(&query, &monsters)))
}

/// Skill names from the client (skillstr.txt), for operations and conditions that name skills.
#[tauri::command]
fn skill_names(ids: Vec<u32>, state: State<'_, AppState>) -> HashMap<u32, String> {
    let Some(resources) = state.resources() else { return HashMap::new() };
    ids.into_iter().filter_map(|id| resources.skill_name(id).map(|name| (id, name))).collect()
}

/// Every shop layout: the user's first, then the built-in ones.
#[tauri::command]
fn gshop_layouts(state: State<'_, AppState>) -> Vec<gshop::Layout> {
    gshop::layout::all(Some(&state.user_dir))
}

/// How `layout` reads a shop file (`path`, or the open shop's file).
#[tauri::command]
async fn preview_gshop_layout(path: Option<String>, layout: gshop::Layout, rows: usize, state: State<'_, AppState>) -> Result<gshop::LayoutPreview, String> {
    let path = match path {
        Some(path) => path,
        None => with_gshop(&state, |document| Ok(document.path().display().to_string()))?,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let data = std::fs::read(&path).map_err(|error| format!("Could not read {path}: {error}"))?;
        gshop::preview(&data, &layout, rows.min(50))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn save_gshop_layout(layout: gshop::Layout, state: State<'_, AppState>) -> Result<String, String> {
    gshop::layout::save(&state.user_dir, &layout).map(|path| path.display().to_string())
}

#[tauri::command]
fn delete_gshop_layout(id: String, state: State<'_, AppState>) -> Result<(), String> {
    gshop::layout::delete(&state.user_dir, &id)
}

/// Problems of the open shop; item and gift templates are checked against the open elements.data.
#[tauri::command]
fn gshop_problems(state: State<'_, AppState>) -> Result<Vec<gshop::Problem>, String> {
    // One lock at a time: the shop's templates, then elements.data, then the check.
    let ids = with_gshop(&state, |document| Ok(document.template_ids()))?;
    let known: Option<HashSet<u32>> = {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        document.as_ref().map(|document| ids.iter().copied().filter(|&id| document.resolve_essence_id(id).is_some()).collect())
    };
    with_gshop(&state, |document| Ok(document.problems(known.as_ref())))
}

/// Opens another npcgen.data read-only (`path`), or compares again with the one already open.
#[tauri::command]
async fn compare_npcgen(path: Option<String>, state: State<'_, AppState>) -> Result<npcgen::GenComparison, String> {
    if let Some(path) = path {
        let other = tauri::async_runtime::spawn_blocking(move || npcgen::ComparedGen::open(path)).await.map_err(|error| error.to_string())??;
        *state.compared_npcgen.lock().map_err(|_| "State lock poisoned")? = Some(other);
    }
    let document = state.npcgen.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_npcgen.lock().map_err(|_| "State lock poisoned")?;
    Ok(document.as_ref().ok_or("Open npcgen.data first")?.compare(compared.as_ref().ok_or("Choose an npcgen.data to compare with")?))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NpcGenCopyResult {
    view: npcgen::View,
    report: npcgen::CopyReport,
}

#[tauri::command]
fn copy_npcgen(picks: Vec<(npcgen::Section, usize)>, state: State<'_, AppState>) -> Result<NpcGenCopyResult, String> {
    let mut document = state.npcgen.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_npcgen.lock().map_err(|_| "State lock poisoned")?;
    let (view, report) = document.as_mut().ok_or("Open npcgen.data first")?.copy_compared(compared.as_ref().ok_or("Choose an npcgen.data to compare with")?, &picks)?;
    Ok(NpcGenCopyResult { view, report })
}

/// Writes a JSON export of the open npcgen.data (all of it, or the picked items and, with `related`, what
/// they use).
#[tauri::command]
fn export_npcgen_json(target: String, picks: Option<Vec<(npcgen::Section, usize)>>, related: bool, state: State<'_, AppState>) -> Result<npcgen::ExportCounts, String> {
    let (json, counts) = state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open npcgen.data first")?.export_json(picks.as_deref(), related)?;
    std::fs::write(&target, json).map_err(|error| format!("Could not write {target}: {error}"))?;
    Ok(counts)
}

#[tauri::command]
fn close_npcgen_comparison(state: State<'_, AppState>) -> Result<(), String> {
    *state.compared_npcgen.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

/// Problems the server would have with the open npcgen.data; templates are checked against the open
/// elements.data, positions against the map size (`half_size`) when the UI knows the map.
#[tauri::command]
fn npcgen_problems(half_size: Option<f32>, state: State<'_, AppState>) -> Result<Vec<npcgen::Problem>, String> {
    // Lock one document at a time: templates first, then the elements lookup, then the check.
    let ids = state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open npcgen.data first")?.template_ids();
    let templates = {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        document.as_ref().map(|document| ids.iter().filter_map(|&id| document.essence_struct(id).map(|(name, _)| (id, name))).collect())
    };
    let context = npcgen::ProblemContext { templates, half_size };
    Ok(state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open npcgen.data first")?.problems(&context))
}

#[tauri::command]
fn import_npcgen_nearby(rows: Vec<npcgen::NearbyImport>, options: npcgen::NearbyOptions, state: State<'_, AppState>) -> Result<npcgen::View, String> {
    state.npcgen.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open npcgen.data first")?.import_nearby(rows, options)
}

/// Image files of the configured client below a package folder (`surfaces\qshop`), for icon pickers.
#[tauri::command]
async fn client_images(folder: String, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let resources = state.resources().ok_or("Set the game client folder in Settings to browse its pictures")?;
    tauri::async_runtime::spawn_blocking(move || resources.images_in(&folder)).await.map_err(|error| error.to_string())?
}

/// The configured client's maps with whether each has a full-map image (for the npcgen.data plot).
#[tauri::command]
async fn client_maps(state: State<'_, AppState>) -> Result<Vec<ClientMap>, String> {
    let resources = state.resources().ok_or("Set the game client folder in Settings to show map images")?;
    tauri::async_runtime::spawn_blocking(move || {
        let maps = resources.instances()?;
        Ok(maps.iter().map(|instance| ClientMap { has_image: resources.has_midmap(&instance.path), instance: instance.clone() }).collect())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// A map's precinct.clt or region.clt (and the other one of its folder, for the map).
#[tauri::command]
async fn open_map_regions(path: String) -> Result<map_regions::Opened, String> {
    tauri::async_runtime::spawn_blocking(move || map_regions::open(std::path::Path::new(&path))).await.map_err(|error| error.to_string())?
}

/// The server copy (precinct.sev / region.sev) of a client file.
#[tauri::command]
async fn read_map_sev(path: String, kind: String) -> Result<map_regions::MapFile, String> {
    tauri::async_runtime::spawn_blocking(move || map_regions::read_sev(std::path::Path::new(&path), &kind)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn map_regions_problems(file: map_regions::MapFile, server: Option<map_regions::MapFile>, instances: Vec<i32>) -> Vec<map_regions::Problem> {
    map_regions::problems(&file, server.as_ref(), &instances)
}

/// What a map file refers to elsewhere: the map's server npcgen.data controllers, domain.data names, and which of
/// `files` (paths below the client's `element` folder) the configured client lacks.
#[tauri::command]
async fn map_region_links(npcgen_path: Option<String>, files: Vec<String>, state: State<'_, AppState>) -> Result<map_regions::Links, String> {
    let resources = state.resources();
    tauri::async_runtime::spawn_blocking(move || {
        let mut links = map_regions::Links::default();
        if let Some(path) = npcgen_path.filter(|path| std::path::Path::new(path).is_file()) {
            match map_regions::controllers_of(std::path::Path::new(&path)) {
                Ok(found) => links.controllers = Some(found),
                Err(error) => links.controllers_error = Some(error),
            }
        }
        if let Some(resources) = resources {
            let domain = resources.element_dir().join("data").join("domain.data");
            if let Ok(file) = domain::open_file(&domain) {
                links.domains = Some(file.data.domains.iter().map(|entry| (entry.id, entry.name.clone())).collect());
            }
            links.missing_files = Some(files.into_iter().filter(|file| !file.trim().is_empty() && !resources.has_file(file)).collect());
        }
        links
    })
    .await
    .map_err(|error| error.to_string())
}

/// Every map of a client `maps` folder against a server `config` folder.
#[tauri::command]
async fn scan_map_regions(client_root: String, server_root: Option<String>, aliases: std::collections::HashMap<String, Vec<String>>, instances: Vec<i32>) -> Result<Vec<map_regions::ScanRow>, String> {
    tauri::async_runtime::spawn_blocking(move || map_regions::scan(std::path::Path::new(&client_root), server_root.as_deref().map(std::path::Path::new), &aliases, &instances)).await.map_err(|error| error.to_string())?
}

/// Writes a server file from its client file, keeping the client's timestamp; returns the backup.
#[tauri::command]
async fn sync_map_server(client_path: String, server_path: String, backup: bool) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || map_regions::sync_server(std::path::Path::new(&client_path), std::path::Path::new(&server_path), backup)).await.map_err(|error| error.to_string())?
}

/// Writes the client file and its server file with one new timestamp.
#[tauri::command]
async fn save_map_regions(path: String, token: Option<String>, file: map_regions::MapFile, server_path: String, backup: bool, replace_changed: bool) -> Result<map_regions::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || map_regions::save(std::path::Path::new(&path), token.as_deref(), &file, std::path::Path::new(&server_path), backup, replace_changed)).await.map_err(|error| error.to_string())?
}

/// zhuxian_question.data (gdeliveryd quiz contest).
#[tauri::command]
async fn open_quiz(path: String) -> Result<quiz::OpenedQuiz, String> {
    tauri::async_runtime::spawn_blocking(move || quiz::open(std::path::Path::new(&path))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn quiz_problems(questions: Vec<quiz::Question>) -> Vec<quiz::Problem> {
    quiz::problems(&questions)
}

#[tauri::command]
async fn save_quiz(path: String, token: Option<String>, questions: Vec<quiz::Question>, backup: bool, replace_changed: bool) -> Result<quiz::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || quiz::save(std::path::Path::new(&path), token.as_deref(), &questions, backup, replace_changed)).await.map_err(|error| error.to_string())?
}

/// Another question file (zhuxian_question.data, JSON or CSV) to compare with or import from.
#[tauri::command]
async fn read_quiz_source(path: String) -> Result<quiz::QuizSource, String> {
    tauri::async_runtime::spawn_blocking(move || quiz::read_source(std::path::Path::new(&path))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn export_quiz(path: String, source_path: String, questions: Vec<quiz::Question>, format: String) -> Result<quiz::ExportReport, String> {
    tauri::async_runtime::spawn_blocking(move || quiz::export(std::path::Path::new(&path), &source_path, &questions, &format)).await.map_err(|error| error.to_string())?
}

/// extra_drops.sev (the server's extra drop tables).
#[tauri::command]
async fn open_extra_drops(path: String, state: State<'_, AppState>) -> Result<extra_drops::OpenedDrops, String> {
    let opened = tauri::async_runtime::spawn_blocking(move || extra_drops::open(std::path::Path::new(&path))).await.map_err(|error| error.to_string())??;
    remember_drops(&state, &opened.path, &opened.file);
    Ok(opened)
}

/// Another extra_drops.sev, read only (compare/copy); the record inspector keeps its file.
#[tauri::command]
async fn read_extra_drops(path: String) -> Result<extra_drops::OpenedDrops, String> {
    tauri::async_runtime::spawn_blocking(move || extra_drops::open(std::path::Path::new(&path))).await.map_err(|error| error.to_string())?
}

/// Names the extra_drops.sev the record inspector reads (the one opened last, at start).
#[tauri::command]
async fn set_extra_drops_source(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let read = path.clone();
    let opened = tauri::async_runtime::spawn_blocking(move || extra_drops::open(std::path::Path::new(&read))).await.map_err(|error| error.to_string())??;
    remember_drops(&state, &path, &opened.file);
    Ok(())
}

/// Keeps the index of a saved or opened extra_drops.sev for the record inspector.
fn remember_drops(state: &State<'_, AppState>, path: &str, file: &extra_drops::DropFile) {
    let path = std::path::PathBuf::from(path);
    let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    if let Ok(mut source) = state.drops_source.lock() {
        *source = Some((path, modified, Arc::new(extra_drops::index(file))));
    }
}

/// The index of the remembered extra_drops.sev, read again when the file changed on disk.
fn drops_index(state: &State<'_, AppState>) -> Option<Arc<extra_drops::DropIndex>> {
    let mut source = state.drops_source.lock().ok()?;
    let (path, modified, index) = source.as_mut()?;
    let now = std::fs::metadata(&*path).and_then(|meta| meta.modified()).ok();
    if now != *modified {
        let file = extra_drops::open(path).ok()?.file;
        *index = Arc::new(extra_drops::index(&file));
        *modified = now;
    }
    Some(index.clone())
}

/// The server's load checks, and monster/item IDs against the open elements.data.
#[tauri::command]
fn extra_drops_problems(file: extra_drops::DropFile, state: State<'_, AppState>) -> Result<Vec<extra_drops::Problem>, String> {
    let mut known = extra_drops::Known::default();
    {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        if let Some(document) = document.as_ref() {
            let monsters: HashSet<u32> = file.tables.iter().flat_map(|table| table.monsters.iter().copied()).collect();
            let items: HashSet<u32> = file.tables.iter().flat_map(|table| extra_drops::used_items(table).iter().map(|item| item.id)).collect();
            known.monsters = Some(monsters.into_iter().filter(|&id| document.essence_struct(id).is_some_and(|(name, _)| name == "MONSTER_ESSENCE")).collect());
            known.items = Some(items.into_iter().filter(|&id| document.resolve_essence_id(id).is_some()).collect());
        }
    }
    Ok(extra_drops::problems(&file, &known))
}

#[tauri::command]
async fn save_extra_drops(path: String, token: Option<String>, file: extra_drops::DropFile, backup: bool, replace_changed: bool, state: State<'_, AppState>) -> Result<extra_drops::SaveReport, String> {
    let (report, file) = tauri::async_runtime::spawn_blocking(move || extra_drops::save(std::path::Path::new(&path), token.as_deref(), &file, backup, replace_changed).map(|report| (report, file))).await.map_err(|error| error.to_string())??;
    remember_drops(&state, &report.path, &file);
    Ok(report)
}

/// Every domain*.data file in the folder of `path` (a domain file or the folder).
#[tauri::command]
async fn open_domains(path: String) -> Result<Vec<domain::DomainFile>, String> {
    tauri::async_runtime::spawn_blocking(move || domain::open_folder(std::path::Path::new(&path))).await.map_err(|error| error.to_string())?
}

/// The configured client's configs\land.txt and landwar.txt rows, by the domain file they override.
#[tauri::command]
async fn domain_land_tables(state: State<'_, AppState>) -> Result<HashMap<String, Vec<domain::LandRow>>, String> {
    let Some(resources) = state.resources() else { return Ok(HashMap::new()) };
    tauri::async_runtime::spawn_blocking(move || domain::land_tables(&resources)).await.map_err(|error| error.to_string())
}

#[tauri::command]
fn domain_problems(name: String, data: domain::DomainData, land: Option<Vec<domain::LandRow>>) -> Vec<domain::Problem> {
    domain::problems(&name, &data, land.as_deref())
}

/// What domain.sev would hold for domain1.data (+ land.txt rows), compared with the target file when it exists.
#[tauri::command]
fn domain_sev_preview(data: domain::DomainData, land: Option<Vec<domain::LandRow>>, time_max: i32, target: Option<String>) -> domain::SevPreview {
    domain::sev_preview(&data, land.as_deref(), time_max, target.as_deref().map(std::path::Path::new))
}

#[tauri::command]
async fn export_domain_sev(sev: domain::Sev, target: String, backup: bool) -> Result<domain::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || domain::export_sev(&sev, std::path::Path::new(&target), backup)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_domains(files: Vec<domain::SaveFile>, backup: bool, replace_changed: bool) -> Result<domain::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || domain::save(&files, backup, replace_changed)).await.map_err(|error| error.to_string())?
}

/// DynamicObjects.data: the path.data reader with that table's rules.
#[tauri::command]
async fn open_dyn_objects(path: String) -> Result<path_data::FileView, String> {
    tauri::async_runtime::spawn_blocking(move || path_data::open_table(path, path_data::Table::DynamicObjects)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn save_dyn_objects(request: path_data::SaveRequest) -> Result<path_data::SaveReport, String> {
    tauri::async_runtime::spawn_blocking(move || path_data::save_table(request, path_data::Table::DynamicObjects)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn export_dyn_objects_json(path: String, source_path: String, rows: Vec<path_data::Row>) -> Result<path_data::JsonReport, String> {
    path_data::export_table_json(path, source_path, rows, path_data::Table::DynamicObjects)
}

#[tauri::command]
fn import_dyn_objects_json(path: String) -> Result<path_data::JsonImport, String> {
    path_data::import_table_json(path, path_data::Table::DynamicObjects)
}

/// Kinds of the rows and whether the configured client has their files.
#[tauri::command]
async fn dyn_object_check(rows: Vec<path_data::Row>, state: State<'_, AppState>) -> Result<Vec<dyn_objects::ObjectInfo>, String> {
    let resources = state.resources();
    tauri::async_runtime::spawn_blocking(move || dyn_objects::check(&rows, resources.as_deref())).await.map_err(|error| error.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DynObjectProblems {
    problems: Vec<dyn_objects::Problem>,
    /// Per row: kind, file and whether the client has it.
    infos: Vec<dyn_objects::ObjectInfo>,
    /// Objects of the open npcgen.data per dynamic object ID.
    uses: HashMap<u32, usize>,
    /// The npcgen.data whose objects were checked (open in its editor).
    npcgen: Option<String>,
    /// Whether files were checked against a configured client.
    client: bool,
}

#[tauri::command]
async fn dyn_object_problems(rows: Vec<path_data::Row>, state: State<'_, AppState>) -> Result<DynObjectProblems, String> {
    let (used, npcgen) = {
        let document = state.npcgen.lock().map_err(|_| "State lock poisoned")?;
        match document.as_ref() {
            Some(document) => (document.dynamic_object_uses(), Some(document.path.display().to_string())),
            None => (HashMap::new(), None),
        }
    };
    let resources = state.resources();
    let client = resources.is_some();
    let (problems, infos, uses) = tauri::async_runtime::spawn_blocking(move || {
        let infos = dyn_objects::check(&rows, resources.as_deref());
        (dyn_objects::problems(&rows, &infos, &used), infos, used)
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(DynObjectProblems { problems, infos, uses, npcgen, client })
}

/// Models or effects of the configured client, for choosing an object's file.
#[tauri::command]
async fn client_dyn_files(kind: String, query: String, offset: usize, limit: usize, state: State<'_, AppState>) -> Result<dyn_objects::FilePage, String> {
    let resources = state.resources().ok_or("Set the game client folder in Settings to browse its models and effects")?;
    tauri::async_runtime::spawn_blocking(move || dyn_objects::client_files(&resources, &kind, &query, offset, limit.min(500))).await.map_err(|error| error.to_string())?
}

/// The configured client's DynamicObjects.data (as saved), for naming npcgen.data's dynamic objects.
#[tauri::command]
fn client_dyn_objects(state: State<'_, AppState>) -> Result<Vec<path_data::Row>, String> {
    let resources = state.resources().ok_or("No game client is configured")?;
    let data = resources.element_dir().join("data");
    let file = std::fs::read_dir(&data)
        .map_err(|error| format!("Could not read {}: {error}", data.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.file_name().is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("dynamicobjects.data")))
        .ok_or("The configured client has no data\\DynamicObjects.data")?;
    Ok(path_data::open_table(file.display().to_string(), path_data::Table::DynamicObjects)?.rows)
}

#[tauri::command]
async fn open_path_data(path: String) -> Result<path_data::FileView, String> {
    tauri::async_runtime::spawn_blocking(move || path_data::open(path)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn open_tasks(path: String, state: State<'_, AppState>) -> Result<tasks::browser::FileSummary, String> {
    let user_dir = state.user_dir.clone();
    let document = tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        if source.supported {
            tasks::browser::TaskDocument::open(path)
        } else {
            let layout = tasks::layout::load(&user_dir, source.version)?.ok_or_else(|| format!("tasks.data v{} has no accepted user layout", source.version))?;
            if !layout.is_verified() {
                return Err(format!("The tasks.data v{} user layout must pass exact verification before it can be opened for editing", source.version));
            }
            tasks::browser::TaskDocument::open_with_schema(path, layout.validate()?)
        }
    })
        .await
        .map_err(|error| error.to_string())??;
    let summary = document.summary();
    *state.tasks.lock().map_err(|_| "State lock poisoned")? = Some(document);
    Ok(summary)
}

#[tauri::command]
async fn inspect_tasks(path: String) -> Result<tasks::analyze::SourceInfo, String> {
    tauri::async_runtime::spawn_blocking(move || tasks::analyze::inspect(path)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn task_source_version(path: String, state: State<'_, AppState>) -> Result<tasks::analyze::SourceVersion, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut source = tasks::analyze::source_version(path)?;
        if !source.supported {
            source.supported = tasks::layout::load(&user_dir, source.version)?.is_some_and(|layout| layout.is_verified());
        }
        Ok(source)
    }).await.map_err(|error| error.to_string())?
}

/// How well each built-in layout reads a sample of an unsupported task set.
#[tauri::command]
async fn probe_task_layouts(path: String) -> Result<Vec<tasks::analyze::LayoutProbe>, String> {
    tauri::async_runtime::spawn_blocking(move || tasks::analyze::probe_layouts(path)).await.map_err(|error| error.to_string())?
}

/// Proposes a layout patch by aligning the open unsupported task set with a supported one.
#[tauri::command]
async fn propose_task_alignment(path: String, reference_path: String) -> Result<tasks::align::AlignProposal, String> {
    tauri::async_runtime::spawn_blocking(move || tasks::align::propose(std::path::Path::new(&path), std::path::Path::new(&reference_path))).await.map_err(|error| error.to_string())?
}

/// Replaces the user layout patch with `operations` on `base_version` and checks every root.
#[tauri::command]
async fn apply_task_alignment(path: String, base_version: u32, operations: Vec<tasks::schema::PatchOperation>, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        if source.supported {
            return Err(format!("tasks.data v{} already has a verified built-in layout", source.version));
        }
        if operations.is_empty() {
            return Err("The proposal has no layout changes; analyze with the reference layout instead".into());
        }
        let mut layout = tasks::layout::UserTaskLayout::new(source.version, base_version)?;
        layout.operations = operations;
        let schema = layout.validate()?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn analyze_tasks(path: String, baseline_version: u32) -> Result<tasks::analyze::AnalysisReport, String> {
    tauri::async_runtime::spawn_blocking(move || tasks::analyze::analyze(path, baseline_version)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn compare_task_ids(path: String, reference_path: String) -> Result<tasks::analyze::IdComparisonReport, String> {
    tauri::async_runtime::spawn_blocking(move || tasks::analyze::compare_ids(path, reference_path)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn score_task_fields(path: String, reference_path: String, state: State<'_, AppState>) -> Result<tasks::analyze::FieldCandidateReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let target = tasks::analyze::source_version(&path)?;
        let reference = tasks::analyze::source_version(&reference_path)?;
        let layout = tasks::layout::load(&user_dir, target.version)?;
        if let Some(layout) = &layout {
            if layout.base_version != reference.version {
                return Err(format!("The v{} user layout is based on v{}, but the selected reference is v{}. Choose a v{} reference task set", target.version, layout.base_version, reference.version, layout.base_version));
            }
        }
        tasks::analyze::score_fixed_fields_with_operations(path, reference_path, layout.as_ref().map(|layout| layout.operations.as_slice()).unwrap_or_default())
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn task_layout_patch(version: u32, state: State<'_, AppState>) -> Result<Option<tasks::layout::LayoutSummary>, String> {
    Ok(tasks::layout::load(&state.user_dir, version)?.map(|layout| tasks::layout::summary(&state.user_dir, &layout)))
}

#[tauri::command]
fn task_schema(version: u32, baseline_version: u32, state: State<'_, AppState>) -> Result<tasks::layout::SchemaView, String> {
    tasks::layout::schema_view(&state.user_dir, version, baseline_version)
}

#[tauri::command]
async fn analyze_task_layout_patch(path: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let layout = tasks::layout::load(&user_dir, source.version)?.ok_or_else(|| format!("No user task layout exists for v{}", source.version))?;
        let schema = layout.validate()?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn verify_task_layout(path: String, baseline_version: u32, state: State<'_, AppState>) -> Result<tasks::layout::LayoutSummary, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        if source.supported {
            return Err(format!("tasks.data v{} already has a verified built-in layout", source.version));
        }
        let mut layout = match tasks::layout::load(&user_dir, source.version)? {
            Some(layout) => {
                if layout.base_version != baseline_version {
                    return Err(format!("The v{} user layout uses baseline v{}, not v{baseline_version}", source.version, layout.base_version));
                }
                layout
            }
            None => tasks::layout::UserTaskLayout::new(source.version, baseline_version)?,
        };
        let schema = layout.validate()?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        if !analysis.exact_round_trip || analysis.exact_roots != analysis.source.root_count || analysis.decoded_bytes != analysis.total_bytes {
            return Err(format!("The v{} layout cannot be accepted: only {} of {} roots and {} of {} bytes decode exactly", source.version, analysis.exact_roots, analysis.source.root_count, analysis.decoded_bytes, analysis.total_bytes));
        }
        // The normal browser performs its own strict pass and must also be able to build every root summary.
        tasks::browser::TaskDocument::open_with_schema(&path, schema)?;
        layout.mark_verified(analysis.exact_roots, analysis.total_bytes)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::summary(&user_dir, &layout))
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn edit_task_layout(version: u32, state: State<'_, AppState>) -> Result<Option<tasks::layout::LayoutSummary>, String> {
    let Some(mut layout) = tasks::layout::load(&state.user_dir, version)? else { return Ok(None) };
    layout.clear_verification();
    tasks::layout::save(&state.user_dir, &layout)?;
    Ok((!layout.operations.is_empty()).then(|| tasks::layout::summary(&state.user_dir, &layout)))
}

#[tauri::command]
async fn add_task_layout_field(path: String, base_version: u32, structure: String, after_field: String, name: String, width: usize, field_type: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = match tasks::layout::load(&user_dir, source.version)? {
            Some(layout) => {
                if layout.base_version != base_version {
                    return Err(format!("The v{} user layout already uses baseline v{}, not v{base_version}", source.version, layout.base_version));
                }
                layout
            }
            None => tasks::layout::UserTaskLayout::new(source.version, base_version)?,
        };
        let schema = layout.add_fixed(structure, after_field, name, width, &field_type)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn add_task_layout_counted_array(path: String, base_version: u32, structure: String, after_field: String, name: String, count_field: String, item_type: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = match tasks::layout::load(&user_dir, source.version)? {
            Some(layout) => {
                if layout.base_version != base_version {
                    return Err(format!("The v{} user layout already uses baseline v{}, not v{base_version}", source.version, layout.base_version));
                }
                layout
            }
            None => tasks::layout::UserTaskLayout::new(source.version, base_version)?,
        };
        let schema = layout.add_counted_array(structure, after_field, name, count_field, item_type)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn remove_task_layout_field(path: String, base_version: u32, structure: String, field: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = match tasks::layout::load(&user_dir, source.version)? {
            Some(layout) => {
                if layout.base_version != base_version {
                    return Err(format!("The v{} user layout already uses baseline v{}, not v{base_version}", source.version, layout.base_version));
                }
                layout
            }
            None => tasks::layout::UserTaskLayout::new(source.version, base_version)?,
        };
        let schema = layout.remove_field(structure, field)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn replace_task_layout_field_type(path: String, base_version: u32, structure: String, field: String, field_type: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = match tasks::layout::load(&user_dir, source.version)? {
            Some(layout) => {
                if layout.base_version != base_version {
                    return Err(format!("The v{} user layout already uses baseline v{}, not v{base_version}", source.version, layout.base_version));
                }
                layout
            }
            None => tasks::layout::UserTaskLayout::new(source.version, base_version)?,
        };
        let schema = layout.replace_field_type(structure, field, &field_type)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn set_task_layout_operation_type(path: String, index: usize, field_type: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = tasks::layout::load(&user_dir, source.version)?.ok_or_else(|| format!("No user task layout exists for v{}", source.version))?;
        let schema = layout.set_type(index, &field_type)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn set_task_layout_operation_conditions(path: String, index: usize, conditions: Vec<tasks::layout::ConditionInput>, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = tasks::layout::load(&user_dir, source.version)?.ok_or_else(|| format!("No user task layout exists for v{}", source.version))?;
        let schema = layout.set_conditions(index, conditions)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn export_task_layout_patch(version: u32, target_path: String, state: State<'_, AppState>) -> Result<tasks::layout::ExportReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || tasks::layout::export(&user_dir, version, &PathBuf::from(target_path))).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_task_layout_patch(tasks_path: String, patch_path: String, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&tasks_path)?;
        if source.supported {
            return Err(format!("tasks.data v{} already has a verified built-in layout", source.version));
        }
        let mut layout = tasks::layout::read(&PathBuf::from(&patch_path))?;
        if layout.task_version != source.version {
            return Err(format!("The imported patch describes tasks.data v{}, but the open file is v{}", layout.task_version, source.version));
        }
        if layout.operations.is_empty() {
            return Err("The imported task-layout patch has no operations".into());
        }
        layout.clear_verification();
        let schema = layout.validate()?;
        let analysis = tasks::analyze::analyze_with_schema(&tasks_path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn remove_task_layout_operation(path: String, index: usize, state: State<'_, AppState>) -> Result<tasks::layout::LayoutReport, String> {
    let user_dir = state.user_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let source = tasks::analyze::source_version(&path)?;
        let mut layout = tasks::layout::load(&user_dir, source.version)?.ok_or_else(|| format!("No user task layout exists for v{}", source.version))?;
        let schema = layout.remove(index)?;
        let analysis = tasks::analyze::analyze_with_schema(&path, &schema, source.version, layout.base_version)?;
        tasks::layout::save(&user_dir, &layout)?;
        Ok(tasks::layout::LayoutReport { patch: tasks::layout::summary(&user_dir, &layout), analysis })
    }).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn get_task(pack: usize, root: usize, path: Vec<usize>, state: State<'_, AppState>) -> Result<tasks::browser::TaskDetail, String> {
    let mut detail = {
        state
            .tasks
            .lock()
            .map_err(|_| "State lock poisoned")?
            .as_mut()
            .ok_or("Open tasks.data first")?
            .task(pack, root, &path)?
    };
    let resources = state.resources();
    let document = state.document.lock().map_err(|_| "State lock poisoned")?;
    detail.resolve_references(document.as_ref(), resources.as_deref());
    Ok(detail)
}

#[tauri::command]
async fn search_tasks(query: String, limit: usize, state: State<'_, AppState>) -> Result<tasks::browser::TaskSearchReport, String> {
    Ok(state
        .tasks
        .lock()
        .map_err(|_| "State lock poisoned")?
        .as_ref()
        .ok_or("Open tasks.data first")?
        .search(&query, limit.min(50_000)))
}

#[tauri::command]
async fn edit_task_field(edit: tasks::browser::FieldEdit, state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.edit_field(edit)
}

#[tauri::command]
async fn edit_task_array(pack: usize, root: usize, task_path: Vec<usize>, array_path: Vec<String>, count_path: Option<Vec<String>>, companions: Option<Vec<Vec<String>>>, edit: tasks::browser::ArrayEdit, state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.edit_array(pack, root, &task_path, &array_path, count_path.as_deref(), &companions.unwrap_or_default(), &edit)
}

#[tauri::command]
async fn edit_task_fields(pack: usize, root: usize, task_path: Vec<usize>, values: Vec<tasks::browser::FieldValue>, label: String, state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.edit_fields(pack, root, &task_path, &values, &label)
}

/// Character classes of the open elements.data, for task class requirements.
#[tauri::command]
async fn character_classes(state: State<'_, AppState>) -> Result<Vec<(u32, String)>, String> {
    Ok(state.document.lock().map_err(|_| "State lock poisoned")?.as_ref().map(|document| document.character_classes()).unwrap_or_default())
}

#[tauri::command]
async fn clone_task_subtree(pack: usize, root: usize, path: Vec<usize>, state: State<'_, AppState>) -> Result<tasks::browser::TaskCloneReport, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.clone_subtask(pack, root, &path)
}

#[tauri::command]
async fn clone_task_root(pack: usize, root: usize, state: State<'_, AppState>) -> Result<tasks::browser::TaskCloneReport, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.clone_root_task(pack, root)
}

#[tauri::command]
async fn task_summary(state: State<'_, AppState>) -> Result<tasks::browser::FileSummary, String> {
    Ok(state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.summary())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn move_task_subtree(source_pack: usize, source_root: usize, source_path: Vec<usize>, source_id: u32, destination_pack: usize, destination_root: usize, destination_path: Vec<usize>, destination_id: u32, state: State<'_, AppState>) -> Result<tasks::browser::TaskMoveReport, String> {
    let mut tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let document = tasks.as_mut().ok_or("Open tasks.data first")?;
    document.check_task_id(source_pack, source_root, &source_path, source_id)?;
    document.check_task_id(destination_pack, destination_root, &destination_path, destination_id)?;
    document.move_subtask(
        source_pack,
        source_root,
        &source_path,
        destination_pack,
        destination_root,
        &destination_path,
    )
}

#[tauri::command]
async fn preview_delete_task_subtree(pack: usize, root: usize, path: Vec<usize>, state: State<'_, AppState>) -> Result<TaskDeletionPreview, String> {
    let preview = state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.delete_subtask_preview(pack, root, &path)?;
    // The task lock is released before the elements document is read.
    let document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let (elements_path, element_uses) = match document.as_ref() {
        Some(document) => (Some(document.path.clone()), preview.lost_ids.iter().flat_map(|id| document.task_id_uses(*id)).collect()),
        None => (None, Vec::new()),
    };
    Ok(TaskDeletionPreview { tasks: preview, elements_path, element_uses })
}

/// A deletion preview with the elements.data places that name the deleted quests.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskDeletionPreview {
    #[serde(flatten)]
    tasks: tasks::browser::TaskDeletePreview,
    elements_path: Option<String>,
    element_uses: Vec<elements::edit::TaskIdUse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskDeletionResult {
    #[serde(flatten)]
    tasks: tasks::browser::TaskDeleteReport,
    elements: Option<elements::edit::EditState>,
    elements_error: Option<String>,
}

#[tauri::command]
/// Deletes a subquest or a top-level quest with everything below it; `element_places` (list, row,
/// offset, quest ID) in the open elements.data are cleared to 0 as one undo step there.
async fn delete_task_subtree(pack: usize, root: usize, path: Vec<usize>, token: String, allow_referenced: bool, element_places: Option<Vec<(usize, usize, usize, u32)>>, state: State<'_, AppState>) -> Result<TaskDeletionResult, String> {
    let report = state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.delete_subtask(pack, root, &path, &token, allow_referenced)?;
    let places = element_places.unwrap_or_default();
    if places.is_empty() {
        return Ok(TaskDeletionResult { tasks: report, elements: None, elements_error: None });
    }
    let mut document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let result = document.as_mut().ok_or_else(|| "elements.data is no longer open".to_string()).and_then(|document| document.clear_task_id_uses(&places));
    Ok(match result {
        Ok(elements) => TaskDeletionResult { tasks: report, elements: Some(elements), elements_error: None },
        Err(error) => TaskDeletionResult { tasks: report, elements: None, elements_error: Some(error) },
    })
}

#[tauri::command]
async fn task_edit_state(state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    Ok(state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.edit_state())
}

#[tauri::command]
async fn task_edit_history(state: State<'_, AppState>) -> Result<Vec<tasks::edit::HistoryEntry>, String> {
    Ok(state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.history())
}

#[tauri::command]
async fn open_task_compare(path: String, state: State<'_, AppState>) -> Result<tasks::compare::TaskCompareReport, String> {
    let user_dir = state.user_dir.clone();
    let compared = tauri::async_runtime::spawn_blocking(move || tasks::compare::ComparedTasks::open(&path, &user_dir))
        .await
        .map_err(|error| error.to_string())??;
    let tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let report = tasks.as_ref().ok_or("Open tasks.data first")?.compare_with(&compared)?;
    *state.compared_tasks.lock().map_err(|_| "State lock poisoned")? = Some(compared);
    Ok(report)
}

/// What a quest ID change touches: references in tasks.data, and quest-ID fields of the open
/// elements.data that hold the old ID.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdChangePreview {
    #[serde(flatten)]
    tasks: tasks::ids::TaskIdChange,
    /// The open elements.data, when one is open.
    elements_path: Option<String>,
    element_uses: Vec<elements::edit::TaskIdUse>,
}

#[tauri::command]
async fn preview_task_id_change(pack: usize, root: usize, task_path: Vec<usize>, expected_id: u32, new_id: u32, state: State<'_, AppState>) -> Result<TaskIdChangePreview, String> {
    let preview = state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.change_task_id(pack, root, &task_path, expected_id, new_id, false)?;
    // The task lock is released before the elements document is read.
    let document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let (elements_path, element_uses) = match document.as_ref() {
        Some(document) => (Some(document.path.clone()), document.task_id_uses(preview.old_id)),
        None => (None, Vec::new()),
    };
    Ok(TaskIdChangePreview { tasks: preview, elements_path, element_uses })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdChangeResult {
    tasks: tasks::edit::EditState,
    /// The elements.data edit state, when places there were changed.
    elements: Option<elements::edit::EditState>,
    /// Why the elements.data part failed (tasks.data was changed already).
    elements_error: Option<String>,
}

/// Gives a quest a new ID, rewriting its references (one undo step in tasks.data) and the chosen
/// elements.data places (`list, row, offset`; one undo step there).
#[tauri::command]
async fn change_task_id(pack: usize, root: usize, task_path: Vec<usize>, expected_id: u32, new_id: u32, element_places: Vec<(usize, usize, usize)>, state: State<'_, AppState>) -> Result<TaskIdChangeResult, String> {
    let change = state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.change_task_id(pack, root, &task_path, expected_id, new_id, true)?;
    let tasks = change.state.clone().ok_or("The ID change was not applied")?;
    if element_places.is_empty() {
        return Ok(TaskIdChangeResult { tasks, elements: None, elements_error: None });
    }
    let mut document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let result = document.as_mut().ok_or_else(|| "elements.data is no longer open".to_string()).and_then(|document| document.replace_task_id_uses(&element_places, change.old_id, change.new_id));
    Ok(match result {
        Ok(elements) => TaskIdChangeResult { tasks, elements: Some(elements), elements_error: None },
        Err(error) => TaskIdChangeResult { tasks, elements: None, elements_error: Some(error) },
    })
}

/// The talks of a task as trees of windows and options.
#[tauri::command]
async fn task_dialogs(pack: usize, root: usize, task_path: Vec<usize>, state: State<'_, AppState>) -> Result<Vec<tasks::dialogs::Dialog>, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.dialogs(pack, root, &task_path)
}

/// Replaces one talk of a task (one undo step).
#[tauri::command]
async fn set_task_dialog(pack: usize, root: usize, task_path: Vec<usize>, dialog: tasks::dialogs::Dialog, label: String, state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.set_dialog(pack, root, &task_path, &dialog, &label)
}

/// The searchable fields of the open task layout.
#[tauri::command]
async fn task_search_fields(state: State<'_, AppState>) -> Result<Vec<tasks::search::SearchField>, String> {
    let tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let document = tasks.as_ref().ok_or("Open tasks.data first")?;
    Ok(tasks::search::field_catalog(&document.schema, document.container.header.version))
}

/// Advanced task search. Reads a snapshot of the task set (unsaved edits included) and scans it
/// without holding the document lock; a newer search or `cancel_task_search` stops it.
#[tauri::command]
async fn search_tasks_advanced(query: tasks::search::TaskQuery, state: State<'_, AppState>) -> Result<tasks::search::TaskSearchResults, String> {
    let control = state.task_search.clone();
    let generation = control.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let source = {
        let tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
        tasks::search::SearchSource::of(tasks.as_ref().ok_or("Open tasks.data first")?)
    };
    tauri::async_runtime::spawn_blocking(move || tasks::search::run(&source, &query, &control, generation))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn cancel_task_search(state: State<'_, AppState>) -> Result<(), String> {
    state.task_search.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
async fn task_search_progress(state: State<'_, AppState>) -> Result<tasks::search::SearchProgress, String> {
    Ok(state.task_search.progress())
}

#[tauri::command]
async fn task_compare(state: State<'_, AppState>) -> Result<tasks::compare::TaskCompareReport, String> {
    let tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_tasks.lock().map_err(|_| "State lock poisoned")?;
    tasks.as_ref().ok_or("Open tasks.data first")?.compare_with(compared.as_ref().ok_or("Choose a task set to compare with first")?)
}

#[tauri::command]
async fn task_compare_fields(id: u32, state: State<'_, AppState>) -> Result<Vec<tasks::compare::FieldDiff>, String> {
    let tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_tasks.lock().map_err(|_| "State lock poisoned")?;
    tasks.as_ref().ok_or("Open tasks.data first")?.compare_task_fields(compared.as_ref().ok_or("Choose a task set to compare with first")?, id)
}

#[tauri::command]
async fn close_task_compare(state: State<'_, AppState>) -> Result<(), String> {
    *state.compared_tasks.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

#[tauri::command]
async fn copy_compared_tasks(selection: tasks::compare::CopySelection, state: State<'_, AppState>) -> Result<tasks::json::ImportReport, String> {
    let mut tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let document = tasks.as_mut().ok_or("Open tasks.data first")?;
    let (rows, same_layout) = {
        let compared = state.compared_tasks.lock().map_err(|_| "State lock poisoned")?;
        let compared = compared.as_ref().ok_or("Choose a task set to compare with first")?;
        (compared.copy_rows(&document.resolve_copy(compared, &selection)?)?, document.same_layout(compared)?)
    };
    document.copy_rows(&rows, same_layout)
}

#[tauri::command]
async fn preview_task_translation(path: String, state: State<'_, AppState>) -> Result<tasks::translate::TranslationReport, String> {
    let user_dir = state.user_dir.clone();
    let source = tauri::async_runtime::spawn_blocking(move || tasks::compare::ComparedTasks::open(&path, &user_dir))
        .await
        .map_err(|error| error.to_string())??;
    let tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let (report, roots) = tasks.as_ref().ok_or("Open tasks.data first")?.translation_plan(&source)?;
    *state.task_translation.lock().map_err(|_| "State lock poisoned")? = Some(tasks::translate::TranslationSource { source, plan: Some((report.token.clone(), roots)) });
    Ok(report)
}

#[tauri::command]
async fn apply_task_translation(token: String, groups: Vec<tasks::translate::TextGroup>, state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    let mut tasks = state.tasks.lock().map_err(|_| "State lock poisoned")?;
    let document = tasks.as_mut().ok_or("Open tasks.data first")?;
    let mut translation = state.task_translation.lock().map_err(|_| "State lock poisoned")?;
    let source = translation.as_mut().ok_or("Preview a translation first")?;
    let current = source.plan.as_ref().is_some_and(|(planned, _)| *planned == token) && document.translation_current(&source.source, &token);
    if !current {
        return Err("The open task set changed since the preview. Preview the translation again.".into());
    }
    let (_, roots) = source.plan.take().unwrap();
    let (edit_state, _) = document.apply_translation(&roots, &groups)?;
    *translation = None;
    Ok(edit_state)
}

#[tauri::command]
async fn close_task_translation(state: State<'_, AppState>) -> Result<(), String> {
    *state.task_translation.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

#[tauri::command]
async fn export_tasks_json(targets: Vec<tasks::json::ExportTarget>, subtrees: bool, path: String, state: State<'_, AppState>) -> Result<tasks::json::ExportReport, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.export_json(&targets, subtrees, std::path::Path::new(&path))
}

#[tauri::command]
async fn import_tasks_json(path: String, token: Option<String>, state: State<'_, AppState>) -> Result<tasks::json::ImportReport, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.import_json(std::path::Path::new(&path), token.as_deref())
}

/// The open tasks.data's task names by ID, once its index is ready (dyn_tasks.data checks against it).
fn quest_names(state: &AppState) -> Option<HashMap<u32, String>> {
    state.tasks.lock().ok()?.as_ref()?.task_names()
}

#[tauri::command]
async fn open_dyn_tasks(path: String, state: State<'_, AppState>) -> Result<dyn_tasks::DynView, String> {
    let document = tauri::async_runtime::spawn_blocking(move || dyn_tasks::DynDocument::open(path)).await.map_err(|error| error.to_string())??;
    let view = document.view();
    *state.dyn_tasks.lock().map_err(|_| "State lock poisoned")? = Some(document);
    Ok(view)
}

#[tauri::command]
fn dyn_tasks_view(state: State<'_, AppState>) -> Result<Option<dyn_tasks::DynView>, String> {
    Ok(state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().map(dyn_tasks::DynDocument::view))
}

#[tauri::command]
fn dyn_task(index: usize, uid: u64, state: State<'_, AppState>) -> Result<dyn_tasks::format::DynTask, String> {
    state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open dyn_tasks.data first")?.task(index, uid)
}

#[tauri::command]
fn set_dyn_task(index: usize, uid: u64, task: dyn_tasks::format::DynTask, label: String, state: State<'_, AppState>) -> Result<dyn_tasks::DynView, String> {
    let quests: Option<HashSet<u32>> = quest_names(&state).map(|names| names.into_keys().collect());
    state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open dyn_tasks.data first")?.set_task(index, uid, task, &label, quests.as_ref())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DynCloneResult {
    view: dyn_tasks::DynView,
    index: usize,
}

#[tauri::command]
fn clone_dyn_task(index: usize, uid: u64, state: State<'_, AppState>) -> Result<DynCloneResult, String> {
    let quests: Option<HashSet<u32>> = quest_names(&state).map(|names| names.into_keys().collect());
    let (view, index) = state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open dyn_tasks.data first")?.clone_task(index, uid, quests.as_ref())?;
    Ok(DynCloneResult { view, index })
}

#[tauri::command]
fn delete_dyn_task(index: usize, uid: u64, state: State<'_, AppState>) -> Result<dyn_tasks::DynView, String> {
    state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open dyn_tasks.data first")?.delete_task(index, uid)
}

#[tauri::command]
fn undo_dyn_task(state: State<'_, AppState>) -> Result<dyn_tasks::DynView, String> {
    state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open dyn_tasks.data first")?.undo()
}

#[tauri::command]
fn redo_dyn_task(state: State<'_, AppState>) -> Result<dyn_tasks::DynView, String> {
    state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open dyn_tasks.data first")?.redo()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DynProblemReport {
    problems: Vec<dyn_tasks::DynProblem>,
    /// Whether task IDs were checked against an open tasks.data, and items against elements.data.
    tasks_checked: bool,
    elements_checked: bool,
}

#[tauri::command]
async fn dyn_task_problems(state: State<'_, AppState>) -> Result<DynProblemReport, String> {
    let quests: Option<HashSet<u32>> = quest_names(&state).map(|names| names.into_keys().collect());
    // One lock at a time: the IDs to check, then elements.data, then the scan.
    let wanted = state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open dyn_tasks.data first")?.element_ids();
    let missing = {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        document.as_ref().map(|document| wanted.into_iter().filter(|id| document.resolve_essence_id(*id).is_none()).collect::<HashSet<_>>())
    };
    let problems = state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open dyn_tasks.data first")?.problems(quests.as_ref(), missing.as_ref());
    Ok(DynProblemReport { problems, tasks_checked: quests.is_some(), elements_checked: missing.is_some() })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DynLabels {
    /// Items and monsters from the open elements.data ("List › name").
    elements: HashMap<u32, String>,
    /// Tasks from the open tasks.data.
    tasks: HashMap<u32, String>,
}

#[tauri::command]
fn dyn_task_labels(elements: Vec<u32>, tasks: Vec<u32>, state: State<'_, AppState>) -> Result<DynLabels, String> {
    let names = quest_names(&state).unwrap_or_default();
    let tasks = tasks.into_iter().filter_map(|id| names.get(&id).map(|name| (id, name.clone()))).collect();
    let document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let elements = match document.as_ref() {
        Some(document) => elements.into_iter().filter_map(|id| document.resolve_essence_id(id).map(|(_, _, label)| (id, label))).collect(),
        None => HashMap::new(),
    };
    Ok(DynLabels { elements, tasks })
}

#[tauri::command]
fn dyn_task_overview(state: State<'_, AppState>) -> Result<Vec<dyn_tasks::DynOverviewRow>, String> {
    Ok(state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open dyn_tasks.data first")?.overview())
}

/// Opens another dyn_tasks.data read-only (`path`), or compares again with the one already open.
#[tauri::command]
async fn compare_dyn_tasks(path: Option<String>, state: State<'_, AppState>) -> Result<dyn_tasks::DynComparison, String> {
    let quests: Option<HashSet<u32>> = quest_names(&state).map(|names| names.into_keys().collect());
    if let Some(path) = path {
        let pack = tauri::async_runtime::spawn_blocking(move || dyn_tasks::ComparedPack::open(path)).await.map_err(|error| error.to_string())??;
        *state.compared_dyn.lock().map_err(|_| "State lock poisoned")? = Some(pack);
    }
    let document = state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_dyn.lock().map_err(|_| "State lock poisoned")?;
    Ok(document.as_ref().ok_or("Open dyn_tasks.data first")?.compare(compared.as_ref().ok_or("Choose a dyn_tasks.data to compare with")?, quests.as_ref()))
}

#[tauri::command]
fn copy_dyn_tasks(ids: Vec<u32>, state: State<'_, AppState>) -> Result<dyn_tasks::DynView, String> {
    let quests: Option<HashSet<u32>> = quest_names(&state).map(|names| names.into_keys().collect());
    let mut document = state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared_dyn.lock().map_err(|_| "State lock poisoned")?;
    document.as_mut().ok_or("Open dyn_tasks.data first")?.copy_from(compared.as_ref().ok_or("Choose a dyn_tasks.data to compare with")?, &ids, quests.as_ref())
}

#[tauri::command]
fn close_dyn_comparison(state: State<'_, AppState>) -> Result<(), String> {
    *state.compared_dyn.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

/// Items or monsters of the open elements.data, for the dyn_tasks.data editor's ID fields.
#[tauri::command]
async fn pick_essence(kind: String, query: String, page: usize, current: Option<u32>, state: State<'_, AppState>) -> Result<elements::picker::ResultPage, String> {
    state.with_document(|doc| {
        let spec = doc.essence_spec(&kind, current)?;
        doc.picker_records(&spec, &query, page)
    })
}

#[tauri::command]
async fn save_dyn_tasks(target: Option<String>, backup: bool, replace_changed: bool, state: State<'_, AppState>) -> Result<dyn_tasks::DynSaveReport, String> {
    state.dyn_tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open dyn_tasks.data first")?.save(target.as_deref(), backup, replace_changed)
}

#[tauri::command]
async fn task_problems(state: State<'_, AppState>) -> Result<tasks::problems::Report, String> {
    // Element IDs are checked without holding both locks (document, then tasks elsewhere).
    let wanted = state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.element_reference_ids()?;
    let missing = {
        let document = state.document.lock().map_err(|_| "State lock poisoned")?;
        document.as_ref().map(|document| wanted.into_iter().filter(|id| document.resolve_essence_id(*id).is_none()).collect::<std::collections::HashSet<_>>())
    };
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.problems(missing.as_ref())
}

#[tauri::command]
async fn task_referenced_by(id: u32, state: State<'_, AppState>) -> Result<Vec<tasks::browser::TaskDeleteReference>, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.referenced_by(id)
}

#[tauri::command]
async fn revert_task_entry(id: u64, state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.revert_entry(id)
}

#[tauri::command]
async fn undo_task_edit(state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.undo()
}

#[tauri::command]
async fn redo_task_edit(state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.redo()
}

#[tauri::command]
async fn revert_task_edits(state: State<'_, AppState>) -> Result<tasks::edit::EditState, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.revert_all()
}

#[tauri::command]
async fn task_save_plan(options: tasks::save::SaveOptions, state: State<'_, AppState>) -> Result<tasks::save::SavePlan, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_ref().ok_or("Open tasks.data first")?.save_plan(&options)
}

#[tauri::command]
async fn save_tasks(options: tasks::save::SaveOptions, state: State<'_, AppState>) -> Result<tasks::save::SaveReport, String> {
    state.tasks.lock().map_err(|_| "State lock poisoned")?.as_mut().ok_or("Open tasks.data first")?.save(&options)
}

#[tauri::command]
async fn save_path_data(request: path_data::SaveRequest, state: State<'_, AppState>) -> Result<path_data::SaveReport, String> {
    let client_path = state.resources().map(|resources| resources.path_data_file());
    let mut report = tauri::async_runtime::spawn_blocking(move || path_data::save(request)).await.map_err(|error| error.to_string())??;
    if client_path.as_deref().is_some_and(|path| path_data::same_path(path, std::path::Path::new(&report.path))) {
        let settings = state.settings.lock().map_err(|_| "State lock poisoned")?.clone();
        state.apply_client(&settings);
        report.client_reloaded = true;
    }
    Ok(report)
}

#[tauri::command]
async fn export_path_data_json(path: String, source_path: String, rows: Vec<path_data::Row>) -> Result<path_data::JsonReport, String> {
    tauri::async_runtime::spawn_blocking(move || path_data::export_json(path, source_path, rows)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_path_data_json(path: String) -> Result<path_data::JsonImport, String> {
    tauri::async_runtime::spawn_blocking(move || path_data::import_json(path)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn list_records(list: usize, state: State<'_, AppState>) -> Result<Vec<RecordRow>, String> {
    state.with_document(|doc| doc.records(list))
}

#[tauri::command]
async fn find_records(query: String, state: State<'_, AppState>) -> Result<elements::FindResult, String> {
    state.with_document(|doc| Ok(doc.find(&query, 200)))
}

#[tauri::command]
async fn picker_search(request: elements::picker::Request, state: State<'_, AppState>) -> Result<elements::picker::ResultPage, String> {
    // Resolve the field while holding the document briefly. Client package
    // parsing can be slower, so it runs without the document lock.
    let spec = state.with_document(|doc| doc.picker_spec(&request))?;
    let page = request.page;
    if matches!(&spec.source, elements::picker::Source::Resource(_)) {
        let resources = state.resources().ok_or("Choose a game client folder in Settings to search this field")?;
        let query = request.query;
        return tauri::async_runtime::spawn_blocking(move || elements::picker::resource_page(&resources, &spec, &query, page))
            .await
            .map_err(|error| error.to_string())?;
    }
    state.with_document(|doc| match &spec.source {
        elements::picker::Source::Records(_) => doc.picker_records(&spec, &request.query, page),
        elements::picker::Source::Dialogs => doc.picker_dialogs(&spec, &request.query, page),
        elements::picker::Source::Resource(_) => unreachable!(),
    })
}

#[tauri::command]
async fn search_records(query: elements::search::Query, state: State<'_, AppState>) -> Result<elements::search::Report, String> {
    state.with_document(|doc| doc.search(&query))
}

#[tauri::command]
async fn search_field_names(list: Option<usize>, state: State<'_, AppState>) -> Result<Vec<elements::search::FieldName>, String> {
    state.with_document(|doc| Ok(doc.field_names(list)))
}

#[tauri::command]
async fn export_records(
    source: elements::export::Source,
    labels: bool,
    path: String,
    state: State<'_, AppState>,
) -> Result<elements::export::Exported, String> {
    state.with_document(|doc| doc.export(&source, labels, &path))
}

/// Previews an import, or applies the preview identified by its token.
#[tauri::command]
async fn import_records(path: String, token: Option<String>, state: State<'_, AppState>) -> Result<elements::import::Report, String> {
    // Read and parse outside the document lock.
    let input = tauri::async_runtime::spawn_blocking(move || elements::import::Input::read(&path)).await.map_err(|e| e.to_string())??;
    state.with_document_mut(|doc| doc.import_records(&input, token.as_deref()))
}

/// Previews UTF-16 text copied by list, record ID and field path from another supported file.
#[tauri::command]
async fn preview_translation(path: String, state: State<'_, AppState>) -> Result<elements::translation::Report, String> {
    let catalog = state.catalog();
    let source = tauri::async_runtime::spawn_blocking(move || Document::open(path, catalog))
        .await
        .map_err(|error| error.to_string())??;
    state.with_document(|target| elements::translation::preview(target, &source))
}

/// Applies the selected lists from an unchanged translation preview.
#[tauri::command]
async fn apply_translation(path: String, token: String, lists: Vec<usize>, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    let catalog = state.catalog();
    let source = tauri::async_runtime::spawn_blocking(move || Document::open(path, catalog))
        .await
        .map_err(|error| error.to_string())??;
    state.with_document_mut(|target| elements::translation::apply(target, &source, &token, &lists))
}

/// Opens a second file to compare the open one with.
#[tauri::command]
async fn open_compare(path: String, state: State<'_, AppState>) -> Result<elements::compare::Summary, String> {
    let catalog = state.catalog();
    let other = tauri::async_runtime::spawn_blocking(move || Document::open(path, catalog))
        .await
        .map_err(|e| e.to_string())??;
    let summary = state.with_document(|doc| Ok(elements::compare::summary(doc, &other)))?;
    *state.compared.lock().map_err(|_| "State lock poisoned")? = Some(other);
    Ok(summary)
}

fn with_compared<T>(state: &AppState, f: impl FnOnce(&Document, &Document) -> T) -> Result<T, String> {
    let doc = state.document.lock().map_err(|_| "State lock poisoned")?;
    let other = state.compared.lock().map_err(|_| "State lock poisoned")?;
    match (doc.as_ref(), other.as_ref()) {
        (Some(a), Some(b)) => Ok(f(a, b)),
        _ => Err("No file to compare with".into()),
    }
}

#[tauri::command]
async fn compare_summary(state: State<'_, AppState>) -> Result<elements::compare::Summary, String> {
    with_compared(&state, elements::compare::summary)
}

#[tauri::command]
async fn compare_list(this: Option<usize>, other: Option<usize>, state: State<'_, AppState>) -> Result<elements::compare::ListDiff, String> {
    with_compared(&state, |a, b| elements::compare::list_diff(a, b, this, other))
}

#[tauri::command]
async fn compare_markdown(other_is_older: bool, state: State<'_, AppState>) -> Result<String, String> {
    with_compared(&state, |a, b| elements::compare::markdown(a, b, other_is_older))
}

/// Copies selected differences from the compared file into the open file.
#[tauri::command]
async fn copy_compare(request: elements::compare::CopyRequest, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    // Keep the documented lock order: open document, then compared document.
    let mut document = state.document.lock().map_err(|_| "State lock poisoned")?;
    let compared = state.compared.lock().map_err(|_| "State lock poisoned")?;
    match (document.as_mut(), compared.as_ref()) {
        (Some(open), Some(other)) => elements::compare::copy_selection(open, other, &request),
        _ => Err("No file to compare with".into()),
    }
}

#[tauri::command]
async fn close_compare(state: State<'_, AppState>) -> Result<(), String> {
    *state.compared.lock().map_err(|_| "State lock poisoned")? = None;
    Ok(())
}

#[tauri::command]
async fn layout_coverage(state: State<'_, AppState>) -> Result<Vec<elements::coverage::CoverageRow>, String> {
    state.with_document(|doc| Ok(doc.coverage()))
}

/// Sets fields of a record (one undo step).
#[tauri::command]
async fn edit_record(list: usize, row: usize, edits: Vec<elements::edit::FieldEdit>, label: String, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| doc.edit(list, row, &edits, &label))
}

/// Copies a record to the end of its list with a new ID.
#[tauri::command]
async fn clone_record(list: usize, row: usize, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| doc.clone_record(list, row))
}

#[tauri::command]
async fn delete_record(list: usize, row: usize, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| doc.delete_record(list, row))
}

/// The open file's summary (list sizes change with clones and deletes).
#[tauri::command]
async fn file_summary(state: State<'_, AppState>) -> Result<FileSummary, String> {
    state.with_document(|doc| Ok(doc.summary()))
}

/// Plans a bulk edit over search results, or with `apply` makes it (one undo step).
#[tauri::command]
async fn bulk_edit(edit: elements::edit::BulkEdit, apply: bool, state: State<'_, AppState>) -> Result<elements::edit::BulkReport, String> {
    state.with_document_mut(|doc| doc.bulk_edit(&edit, apply))
}

#[tauri::command]
async fn undo_edit(state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| Ok(doc.undo()))
}

#[tauri::command]
async fn redo_edit(state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| Ok(doc.redo()))
}

/// Puts records (or, without any, every changed one) back as opened.
#[tauri::command]
async fn revert_edits(records: Option<Vec<(usize, usize)>>, label: String, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| Ok(doc.revert(records.as_deref(), &label)))
}

#[tauri::command]
async fn revert_talk(index: usize, label: String, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| doc.revert_talk(index, &label))
}

#[tauri::command]
async fn edit_history(state: State<'_, AppState>) -> Result<Vec<elements::edit::HistoryEntry>, String> {
    state.with_document(|doc| Ok(doc.history()))
}

/// Takes back one edit of the history; `force` overwrites later edits of the same fields.
#[tauri::command]
async fn revert_history_entry(id: u64, force: bool, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| doc.revert_entry(id, force))
}

/// What saving to a path would do: changes, checksum, backup.
#[tauri::command]
async fn save_plan(options: elements::save::SaveOptions, state: State<'_, AppState>) -> Result<elements::save::SavePlan, String> {
    state.with_document(|doc| doc.save_plan(&options))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    report: elements::save::SaveReport,
    summary: FileSummary,
    state: elements::edit::EditState,
}

/// Writes the open file (with its edits) to a path; it then is the open file.
#[tauri::command]
async fn save_elements(options: elements::save::SaveOptions, state: State<'_, AppState>) -> Result<Saved, String> {
    state.with_document_mut(|doc| {
        let report = doc.save(&options)?;
        Ok(Saved { report, summary: doc.summary(), state: doc.edit_state() })
    })
}

#[tauri::command]
async fn edit_state(state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document(|doc| Ok(doc.edit_state()))
}

#[tauri::command]
async fn list_problems(state: State<'_, AppState>) -> Result<elements::problems::Report, String> {
    state.with_document(|doc| Ok(doc.problems()))
}

#[tauri::command]
async fn list_talks(state: State<'_, AppState>) -> Result<Vec<elements::TalkSummary>, String> {
    state.with_document(|doc| doc.talks())
}

#[tauri::command]
async fn get_talk(index: usize, state: State<'_, AppState>) -> Result<elements::TalkDetail, String> {
    state.with_document(|doc| doc.talk(index))
}

#[tauri::command]
async fn edit_talk_text(index: usize, edit: elements::edit::TalkTextEdit, state: State<'_, AppState>) -> Result<elements::edit::EditState, String> {
    state.with_document_mut(|doc| doc.edit_talk_text(index, &edit))
}

#[tauri::command]
async fn referenced_by(list: usize, row: usize, state: State<'_, AppState>) -> Result<elements::refs::ReferencedBy, String> {
    state.with_document(|doc| doc.referenced_by(list, row))
}

#[tauri::command]
async fn get_record(list: usize, index: usize, state: State<'_, AppState>) -> Result<RecordDetail, String> {
    let (mut detail, monsters) = state.with_document(|doc| Ok((doc.record(list, index)?, doc.list_struct(list).is_some_and(|name| name.eq_ignore_ascii_case("MONSTER_ESSENCE")))))?;
    // The record's ID: its extra drop table (monsters) or the tables that drop it (items).
    if let Some(drops) = drops_index(&state) {
        if let Some(node) = detail.nodes.iter_mut().find(|node| node.name.eq_ignore_ascii_case("id")) {
            if let Some(hint) = node.value.as_deref().and_then(|value| value.parse::<u32>().ok()).and_then(|id| extra_drops::hint(&drops, id, monsters)) {
                node.hint = Some(hint);
                node.display = Some("drops".into());
            }
        }
    }
    // Fields with the aipolicy role: the policy they name, when aipolicy.data is open (locked after the document).
    if let Ok(policies) = state.aipolicy.lock() {
        if let Some(policies) = policies.as_ref() {
            annotate_policies(&mut detail.nodes, policies);
        }
    }
    Ok(detail)
}

fn annotate_policies(nodes: &mut [elements::decode::Node], policies: &aipolicy::Document) {
    for node in nodes {
        if node.display.as_deref() == Some("aipolicy") {
            let id: i64 = node.value.as_deref().and_then(|value| value.parse().ok()).unwrap_or(0);
            node.hint = Some(match u32::try_from(id).ok().filter(|&id| id != 0).map(|id| (id, policies.policy_by_id(id))) {
                None => "no policy".into(),
                Some((_, Some((_, triggers)))) => format!("AI policy · {triggers} trigger{}", if triggers == 1 { "" } else { "s" }),
                Some((id, None)) => format!("not in aipolicy.data: the server drops policy {id}"),
            });
        }
        if let Some(children) = node.children.as_mut() {
            annotate_policies(children, policies);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EnumInfo {
    key: String,
    label: String,
    flags: bool,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaContext {
    enums: Vec<EnumInfo>,
    /// Lists of the open file that refs can point at.
    targets: Vec<elements::RefTarget>,
    user_dir: String,
    errors: Vec<String>,
}

#[tauri::command]
async fn schema_context(state: State<'_, AppState>) -> Result<SchemaContext, String> {
    let user_dir = state.user_dir.join("layouts").display().to_string();
    state.with_document(|doc| {
        let catalog = doc.catalog();
        let mut enums: Vec<EnumInfo> = catalog
            .enums
            .iter()
            .map(|(key, set)| EnumInfo {
                key: key.clone(),
                label: if set.label.is_empty() { key.clone() } else { set.label.clone() },
                flags: set.flags,
                count: set.items.len(),
            })
            .collect();
        enums.sort_by(|a, b| a.label.cmp(&b.label));
        Ok(SchemaContext { enums, targets: doc.ref_targets(), user_dir, errors: catalog.errors.clone() })
    })
}

#[tauri::command]
async fn get_list_schema(list: usize, state: State<'_, AppState>) -> Result<ListSchema, String> {
    state.with_document(|doc| doc.list_schema(list))
}

#[tauri::command]
async fn preview_list_schema(list: usize, index: usize, def: ListDef, state: State<'_, AppState>) -> Result<RecordDetail, String> {
    state.with_document(|doc| doc.preview(list, index, &def))
}

#[tauri::command]
async fn save_list_schema(list: usize, def: ListDef, state: State<'_, AppState>) -> Result<FileSummary, String> {
    def.check()?;
    let dir = state.user_dir.join("layouts");
    state.change_schemas(|doc| {
        let item_size = doc.file.lists.get(list).ok_or("No such list")?.item_size;
        if let Some(size) = def.size.filter(|&s| s > item_size) {
            return Err(format!("The schema describes {size} bytes, but records of this list are {item_size} bytes"));
        }
        save_user_list(&dir, &doc.edit_target(), list, &def, doc.new_target_meta().as_ref())
    })
}

#[tauri::command]
async fn import_candidates(list: usize, state: State<'_, AppState>) -> Result<Vec<ImportCandidate>, String> {
    state.with_document(|doc| doc.import_candidates(list))
}

/// Compares one target list with an exact reference file, asks the configured
/// model for a schema proposal, and validates it. The proposal is returned to
/// the schema editor and is not saved here.
#[tauri::command]
async fn analyze_list_layout(reference_path: String, source_layout: String, list: usize, state: State<'_, AppState>) -> Result<elements::analyze::LayoutAnalysis, String> {
    let (endpoint, model, api_key) = {
        let settings = state.settings.lock().map_err(|_| "State lock poisoned")?;
        let (endpoint, model, api_key) = settings.ai().ok_or("Configure an AI endpoint, model and API key in Settings first")?;
        (endpoint.to_string(), model.to_string(), api_key.to_string())
    };
    let catalog = state.catalog.read().map_err(|_| "State lock poisoned")?.clone();
    let reference = Document::open(reference_path.trim().to_string(), catalog)?;
    let (target_path, target_version, prompt, reference_list, matched_records, target_size) = state.with_document(|doc| {
        let (prompt, reference_list, matched_records, target_size) = elements::analyze::prompt(doc, &reference, list, Some(&source_layout))?;
        Ok((doc.path.clone(), doc.file.version(), prompt, reference_list, matched_records, target_size))
    })?;

    let analysis = elements::analyze::request(&endpoint, &model, &api_key, prompt, reference_list, matched_records, target_size).await?;

    // The request can take a while. Refuse to apply its result to a different
    // file/list if the user changed documents while it was running.
    state.with_document(|doc| {
        let block = doc.file.lists.get(list).ok_or("The target list is no longer open")?;
        if doc.path != target_path || doc.file.version() != target_version || block.item_size != target_size {
            return Err("The open file changed while the AI was analyzing it. Run the analysis again.".into());
        }
        for row in 0..block.count.min(8) {
            doc.preview(list, row, &analysis.definition)?;
        }
        Ok(analysis)
    })
}

/// Aligns an exact older schema to the open list using matching record bytes.
/// This is fully local and marks unrecognised spans as raw bytes.
#[tauri::command]
async fn analyze_list_from_reference(reference_path: String, source_layout: String, list: usize, state: State<'_, AppState>) -> Result<elements::analyze::LayoutAnalysis, String> {
    let catalog = state.catalog.read().map_err(|_| "State lock poisoned")?.clone();
    let reference = Document::open(reference_path.trim().to_string(), catalog)?;
    state.with_document(|doc| elements::analyze::from_reference(doc, &reference, list, &source_layout))
}

#[tauri::command]
async fn reset_list_schema(list: usize, state: State<'_, AppState>) -> Result<FileSummary, String> {
    let dir = state.user_dir.join("layouts");
    state.change_schemas(|doc| delete_user_list(&dir, &doc.edit_target(), list))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    settings: Settings,
    /// What was found in the client folder, if one is set and readable.
    client: Option<ClientInfo>,
    client_error: Option<String>,
    icon_generation: u32,
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<SettingsView, String> {
    let settings = state.settings.lock().map_err(|_| "State lock poisoned")?.clone();
    let (client, error) = match settings.client_dir() {
        Some(dir) => match client::inspect(std::path::Path::new(dir)) {
            Ok(info) => (Some(info), None),
            Err(e) => (None, Some(e)),
        },
        None => (None, None),
    };
    Ok(state.settings_view(client, error))
}

/// Checks a client folder without saving it (for the settings dialog).
#[tauri::command]
async fn inspect_client(dir: String) -> Result<ClientInfo, String> {
    client::inspect(std::path::Path::new(dir.trim()))
}

#[tauri::command]
async fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<SettingsView, String> {
    if let Some(dir) = settings.client_dir() {
        client::inspect(std::path::Path::new(dir))?;
    }
    settings.save(&state.settings_path)?;
    *state.settings.lock().map_err(|_| "State lock poisoned")? = settings.clone();
    let (client, error) = state.apply_client(&settings);
    Ok(state.settings_view(client, error))
}

/// The path ID in an icon URL path: `/<generation>-<path id>` (also accepts
/// `/<generation>/<path id>`, encoded or not).
fn icon_id(uri_path: &str) -> Option<u32> {
    uri_path.replace("%2F", "/").replace("%2f", "/").rsplit(['/', '-']).next()?.parse().ok()
}

/// `jdicon://localhost/<generation>-<path id>` → the item icon as a PNG.
fn icon_response(app: &tauri::AppHandle, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let Some(id) = icon_id(uri_path) else { return not_found() };
    let Some(res) = app.state::<AppState>().resources() else { return not_found() };
    match res.item_icon_png(id) {
        Ok(png) => tauri::http::Response::builder()
            .header("Content-Type", "image/png")
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(png.to_vec())
            .unwrap(),
        Err(_) => not_found(),
    }
}

/// Decodes %XX escapes (UTF-8) in a URL path segment.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let Some(value) = std::str::from_utf8(&bytes[at + 1..at + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(value);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `jdres://localhost/<generation>-<package path>` → an image of a client package by its path (shop icons).
fn resource_response(app: &tauri::AppHandle, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let decoded = percent_decode(uri_path.trim_start_matches('/'));
    let Some((_, path)) = decoded.split_once('-') else { return not_found() };
    if path.contains("..") {
        return not_found();
    }
    let Some(res) = app.state::<AppState>().resources() else { return not_found() };
    match res.image_at(path) {
        Ok(image) => tauri::http::Response::builder()
            .header("Content-Type", image.content_type)
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(image.bytes.clone())
            .unwrap(),
        Err(_) => not_found(),
    }
}

/// `jdmap://localhost/<generation>-<map path>` → the map's midmap as a PNG.
fn map_response(app: &tauri::AppHandle, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let decoded = percent_decode(uri_path.trim_start_matches('/'));
    let Some((_, path)) = decoded.split_once('-') else { return not_found() };
    let Some(res) = app.state::<AppState>().resources() else { return not_found() };
    match res.midmap_png(path) {
        Ok(png) => tauri::http::Response::builder()
            .header("Content-Type", "image/png")
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(png.to_vec())
            .unwrap(),
        Err(_) => not_found(),
    }
}

/// `jdimage://localhost/<generation>-<path id>` → a standalone client image.
fn image_response(app: &tauri::AppHandle, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || tauri::http::Response::builder().status(404).body(Vec::new()).unwrap();
    let Some(id) = icon_id(uri_path) else { return not_found() };
    let Some(res) = app.state::<AppState>().resources() else { return not_found() };
    match res.image(id) {
        Ok(image) => tauri::http::Response::builder()
            .header("Content-Type", image.content_type)
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(image.bytes.clone())
            .unwrap(),
        Err(_) => not_found(),
    }
}

// ---------------------------------------------------------------- enums and masks

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetSummary {
    key: String,
    label: String,
    kind: SetKind,
    origin: SetOrigin,
    /// Number of values or bits.
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetDetail {
    kind: SetKind,
    set: NamedSet,
    origin: SetOrigin,
    /// The built-in definition, for sets that have one.
    builtin: Option<NamedSet>,
    /// Fields that use the set, across all layouts.
    usage: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SetsChanged {
    sets: Vec<SetSummary>,
    /// The open file, re-read with the new sets.
    summary: Option<FileSummary>,
}

fn set_summaries(catalog: &Catalog) -> Vec<SetSummary> {
    let mut out: Vec<SetSummary> = catalog
        .sets
        .values()
        .chain(catalog.deleted.values())
        .map(|e| SetSummary {
            key: e.set.key.clone(),
            label: if e.set.label.is_empty() { e.set.key.clone() } else { e.set.label.clone() },
            kind: e.kind,
            origin: e.origin,
            count: e.set.values.len() + e.set.flags.len(),
        })
        .collect();
    out.sort_by(|a, b| (a.kind as u8, a.label.to_lowercase()).cmp(&(b.kind as u8, b.label.to_lowercase())));
    out
}

#[tauri::command]
async fn named_sets(state: State<'_, AppState>) -> Result<Vec<SetSummary>, String> {
    Ok(set_summaries(&state.catalog()))
}

#[tauri::command]
async fn named_set(key: String, state: State<'_, AppState>) -> Result<SetDetail, String> {
    let catalog = state.catalog();
    let entry = catalog.sets.get(&key).ok_or_else(|| format!("No enum or mask named {key:?}"))?;
    Ok(SetDetail {
        kind: entry.kind,
        set: entry.set.clone(),
        origin: entry.origin,
        builtin: builtin_set(&key).map(|b| b.set.clone()),
        usage: catalog.set_usage(&key),
    })
}

#[tauri::command]
async fn save_named_set(kind: SetKind, set: NamedSet, state: State<'_, AppState>) -> Result<SetsChanged, String> {
    if let Some(existing) = state.catalog().sets.get(&set.key) {
        if existing.kind != kind {
            return Err(format!("{:?} is already used by a {}", set.key, existing.kind.folder().trim_end_matches('s')));
        }
    }
    save_user_set(&state.user_dir, kind, &set)?;
    let summary = state.reload_catalog()?;
    Ok(SetsChanged { sets: set_summaries(&state.catalog()), summary })
}

#[tauri::command]
async fn delete_named_set(key: String, state: State<'_, AppState>) -> Result<SetsChanged, String> {
    delete_set(&state.user_dir, &key)?;
    let summary = state.reload_catalog()?;
    Ok(SetsChanged { sets: set_summaries(&state.catalog()), summary })
}

/// Drops the user's version of a built-in set: reverts an edit, restores a deleted one.
#[tauri::command]
async fn revert_named_set(key: String, state: State<'_, AppState>) -> Result<SetsChanged, String> {
    delete_user_set(&state.user_dir, &key)?;
    let summary = state.reload_catalog()?;
    Ok(SetsChanged { sets: set_summaries(&state.catalog()), summary })
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_urls_parse_with_or_without_encoding() {
        assert_eq!(super::icon_id("/3-1291"), Some(1291));
        assert_eq!(super::icon_id("/3%2F1291"), Some(1291));
        assert_eq!(super::icon_id("/3/1291"), Some(1291));
        assert_eq!(super::icon_id("/favicon.ico"), None);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let user_dir = app.path().app_data_dir()?;
            let catalog = Arc::new(Catalog::load(Some(&user_dir)));
            for error in &catalog.errors {
                eprintln!("user layout skipped: {error}");
            }
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path);
            let state = AppState {
                document: Mutex::new(None),
                tasks: Mutex::new(None),
                dyn_tasks: Mutex::new(None),
                compared_dyn: Mutex::new(None),
                npcgen: Mutex::new(None),
                gshop: Mutex::new(None),
                compared_gshop: Mutex::new(None),
                aipolicy: Mutex::new(None),
                drops_source: Mutex::new(None),
                compared_aipolicy: Mutex::new(None),
                compared_npcgen: Mutex::new(None),
                compared: Mutex::new(None),
                compared_tasks: Mutex::new(None),
                task_translation: Mutex::new(None),
                task_search: Arc::new(tasks::search::SearchControl::default()),
                catalog: RwLock::new(catalog),
                user_dir,
                settings: Mutex::new(settings.clone()),
                settings_path,
                resources: RwLock::new(None),
                icon_generation: AtomicU32::new(0),
            };
            let (_, error) = state.apply_client(&settings);
            if let Some(error) = error {
                eprintln!("client folder: {error}");
            }
            app.manage(state);
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("jdicon", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(icon_response(&app, &path)));
        })
        .register_asynchronous_uri_scheme_protocol("jdimage", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(image_response(&app, &path)));
        })
        .register_asynchronous_uri_scheme_protocol("jdres", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(resource_response(&app, &path)));
        })
        .register_asynchronous_uri_scheme_protocol("jdmap", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(map_response(&app, &path)));
        })
        .invoke_handler(tauri::generate_handler![
            open_elements,
            open_path_data,
            open_tasks,
            task_source_version,
            inspect_tasks,
            analyze_tasks,
            compare_task_ids,
            score_task_fields,
            task_layout_patch,
            task_schema,
            analyze_task_layout_patch,
            verify_task_layout,
            edit_task_layout,
            add_task_layout_field,
            add_task_layout_counted_array,
            remove_task_layout_field,
            replace_task_layout_field_type,
            set_task_layout_operation_type,
            set_task_layout_operation_conditions,
            export_task_layout_patch,
            import_task_layout_patch,
            remove_task_layout_operation,
            get_task,
            search_tasks,
            edit_task_field,
            clone_task_subtree,
            clone_task_root,
            edit_task_fields,
            edit_task_array,
            task_search_fields,
            probe_task_layouts,
            open_dyn_tasks,
            open_task_npc,
            open_npcgen,
            npcgen_view,
            npcgen_item,
            set_npcgen_item,
            clone_npcgen_item,
            delete_npcgen_item,
            undo_npcgen,
            redo_npcgen,
            save_npcgen,
            save_task_npc,
            client_map_names,
            client_maps,
            client_images,
            game_clients,
            game_position,
            game_nearby,
            import_npcgen_nearby,
            npcgen_problems,
            compare_npcgen,
            copy_npcgen,
            close_npcgen_comparison,
            export_npcgen_json,
            open_gshop,
            gshop_view,
            gshop_item,
            set_gshop_item,
            clone_gshop_item,
            delete_gshop_item,
            move_gshop_item,
            edit_gshop_categories,
            undo_gshop,
            redo_gshop,
            save_gshop,
            gshop_problems,
            compare_gshop,
            gshop_text_updates,
            apply_gshop_texts,
            copy_gshop,
            close_gshop_comparison,
            export_gshop_json,
            gshop_layouts,
            open_aipolicy,
            aipolicy_view,
            aipolicy_policy,
            aipolicy_used_by,
            aipolicy_search,
            aipolicy_set_trigger,
            aipolicy_trigger_action,
            aipolicy_policy_action,
            aipolicy_unused_subtriggers,
            aipolicy_cleanup,
            open_map_regions,
            read_map_sev,
            map_regions_problems,
            save_map_regions,
            scan_map_regions,
            map_region_links,
            sync_map_server,
            open_quiz,
            quiz_problems,
            save_quiz,
            read_quiz_source,
            export_quiz,
            open_extra_drops,
            set_extra_drops_source,
            read_extra_drops,
            extra_drops_problems,
            save_extra_drops,
            open_domains,
            domain_land_tables,
            domain_problems,
            save_domains,
            domain_sev_preview,
            export_domain_sev,
            open_dyn_objects,
            save_dyn_objects,
            export_dyn_objects_json,
            import_dyn_objects_json,
            dyn_object_check,
            dyn_object_problems,
            client_dyn_files,
            client_dyn_objects,
            aipolicy_history,
            aipolicy_revert_entry,
            aipolicy_revert_all,
            compare_aipolicy,
            aipolicy_compare_policy,
            copy_aipolicy,
            close_aipolicy_comparison,
            export_aipolicy_json,
            aipolicy_translation,
            apply_aipolicy_texts,
            monster_policy_slots,
            set_monster_policies,
            aipolicy_undo,
            aipolicy_redo,
            aipolicy_save,
            aipolicy_problems,
            aipolicy_catalog,
            pick_skill,
            skill_names,
            open_vipaward,
            save_vipaward,
            vipaward_problems,
            vipaward_level_names,
            propose_gshop_layout,
            preview_gshop_layout,
            save_gshop_layout,
            delete_gshop_layout,
            dyn_tasks_view,
            dyn_task,
            set_dyn_task,
            clone_dyn_task,
            delete_dyn_task,
            undo_dyn_task,
            redo_dyn_task,
            dyn_task_problems,
            dyn_task_labels,
            save_dyn_tasks,
            dyn_task_overview,
            compare_dyn_tasks,
            copy_dyn_tasks,
            close_dyn_comparison,
            pick_essence,
            propose_task_alignment,
            apply_task_alignment,
            task_dialogs,
            preview_task_id_change,
            change_task_id,
            set_task_dialog,
            search_tasks_advanced,
            cancel_task_search,
            task_search_progress,
            character_classes,
            task_summary,
            revert_task_entry,
            task_problems,
            task_referenced_by,
            export_tasks_json,
            preview_task_translation,
            apply_task_translation,
            close_task_translation,
            open_task_compare,
            task_compare,
            task_compare_fields,
            close_task_compare,
            copy_compared_tasks,
            import_tasks_json,
            move_task_subtree,
            preview_delete_task_subtree,
            delete_task_subtree,
            task_edit_state,
            task_edit_history,
            undo_task_edit,
            redo_task_edit,
            revert_task_edits,
            task_save_plan,
            save_tasks,
            save_path_data,
            export_path_data_json,
            import_path_data_json,
            list_records,
            get_record,
            referenced_by,
            schema_context,
            get_list_schema,
            preview_list_schema,
            save_list_schema,
            reset_list_schema,
            import_candidates,
            analyze_list_layout,
            analyze_list_from_reference,
            get_settings,
            inspect_client,
            save_settings,
            find_records,
            picker_search,
            list_problems,
            edit_record,
            bulk_edit,
            clone_record,
            delete_record,
            file_summary,
            undo_edit,
            redo_edit,
            revert_edits,
            revert_talk,
            edit_state,
            save_plan,
            save_elements,
            edit_history,
            revert_history_entry,
            layout_coverage,
            export_records,
            import_records,
            preview_translation,
            apply_translation,
            open_compare,
            compare_summary,
            compare_list,
            compare_markdown,
            copy_compare,
            close_compare,
            search_records,
            search_field_names,
            list_talks,
            get_talk,
            edit_talk_text,
            named_sets,
            named_set,
            save_named_set,
            delete_named_set,
            revert_named_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
