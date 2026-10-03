mod client;
mod elements;
mod settings;

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
    /// A second file compared with the open one (locked after `document`).
    compared: Mutex<Option<Document>>,
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
async fn list_records(list: usize, state: State<'_, AppState>) -> Result<Vec<RecordRow>, String> {
    state.with_document(|doc| doc.records(list))
}

#[tauri::command]
async fn find_records(query: String, state: State<'_, AppState>) -> Result<elements::FindResult, String> {
    state.with_document(|doc| Ok(doc.find(&query, 200)))
}

#[tauri::command]
async fn search_records(query: elements::search::Query, state: State<'_, AppState>) -> Result<elements::search::Report, String> {
    state.with_document(|doc| doc.search(&query))
}

#[tauri::command]
async fn search_field_names(state: State<'_, AppState>) -> Result<Vec<elements::search::FieldName>, String> {
    state.with_document(|doc| Ok(doc.field_names()))
}

#[tauri::command]
async fn export_records(
    source: elements::export::Source,
    format: elements::export::Format,
    labels: bool,
    path: String,
    state: State<'_, AppState>,
) -> Result<elements::export::Exported, String> {
    state.with_document(|doc| doc.export(&source, format, labels, &path))
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
async fn referenced_by(list: usize, row: usize, state: State<'_, AppState>) -> Result<elements::refs::ReferencedBy, String> {
    state.with_document(|doc| doc.referenced_by(list, row))
}

#[tauri::command]
async fn get_record(list: usize, index: usize, state: State<'_, AppState>) -> Result<RecordDetail, String> {
    state.with_document(|doc| doc.record(list, index))
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
                compared: Mutex::new(None),
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
        .invoke_handler(tauri::generate_handler![
            open_elements,
            list_records,
            get_record,
            referenced_by,
            schema_context,
            get_list_schema,
            preview_list_schema,
            save_list_schema,
            reset_list_schema,
            import_candidates,
            get_settings,
            inspect_client,
            save_settings,
            find_records,
            list_problems,
            edit_record,
            bulk_edit,
            clone_record,
            delete_record,
            file_summary,
            undo_edit,
            redo_edit,
            revert_edits,
            edit_state,
            save_plan,
            save_elements,
            edit_history,
            revert_history_entry,
            layout_coverage,
            export_records,
            open_compare,
            compare_summary,
            compare_list,
            compare_markdown,
            close_compare,
            search_records,
            search_field_names,
            list_talks,
            get_talk,
            named_sets,
            named_set,
            save_named_set,
            delete_named_set,
            revert_named_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
