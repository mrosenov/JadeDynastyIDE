mod elements;

use std::sync::Mutex;

use elements::{Document, FileSummary, RecordDetail, RecordRow};
use tauri::State;

#[derive(Default)]
struct AppState {
    document: Mutex<Option<Document>>,
}

fn with_document<T>(state: &AppState, f: impl FnOnce(&Document) -> Result<T, String>) -> Result<T, String> {
    let guard = state.document.lock().map_err(|_| "State lock poisoned")?;
    let doc = guard.as_ref().ok_or("No file is open")?;
    f(doc)
}

#[tauri::command]
async fn open_elements(path: String, state: State<'_, AppState>) -> Result<FileSummary, String> {
    let doc = tauri::async_runtime::spawn_blocking(move || Document::open(path))
        .await
        .map_err(|e| e.to_string())??;
    let summary = doc.summary();
    *state.document.lock().map_err(|_| "State lock poisoned")? = Some(doc);
    Ok(summary)
}

#[tauri::command]
async fn list_records(list: usize, state: State<'_, AppState>) -> Result<Vec<RecordRow>, String> {
    with_document(&state, |doc| doc.records(list))
}

#[tauri::command]
async fn get_record(list: usize, index: usize, state: State<'_, AppState>) -> Result<RecordDetail, String> {
    with_document(&state, |doc| doc.record(list, index))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![open_elements, list_records, get_record])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
