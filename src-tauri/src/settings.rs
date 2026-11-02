//! User settings, stored as `settings.json` in the app config folder.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The game client folder (or its `element` folder).
    pub client_dir: Option<String>,
    /// Open the client's elements.data when the app starts.
    pub open_on_start: bool,
    /// Application colours: follow the operating system, or force light/dark.
    pub theme: Theme,
    /// Interface translation catalog. Unknown values fall back to English in the UI.
    pub language: Option<String>,
    /// Full URL of an OpenAI-compatible Responses or Chat Completions endpoint.
    pub ai_endpoint: Option<String>,
    /// Model used for layout analysis.
    pub ai_model: Option<String>,
    /// API key sent only to the configured endpoint.
    pub ai_api_key: Option<String>,
}

impl Settings {
    /// Reads the settings; missing or unreadable settings give the defaults.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("Could not write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("Could not write {}: {e}", path.display()))
    }

    /// The client folder, with blank values treated as unset.
    pub fn client_dir(&self) -> Option<&str> {
        self.client_dir.as_deref().map(str::trim).filter(|d| !d.is_empty())
    }

    /// Config needed before the layout analyzer is exposed in the UI.
    pub fn ai(&self) -> Option<(&str, &str, &str)> {
        let endpoint = self.ai_endpoint.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
        let model = self.ai_model.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
        let key = self.ai_api_key.as_deref().map(str::trim).filter(|s| !s.is_empty())?;
        Some((endpoint, model, key))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocaleCatalog {
    pub code: String,
    pub name: String,
    pub path: String,
    pub messages: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocaleCatalogs {
    pub directory: String,
    pub template_path: String,
    pub locales: Vec<LocaleCatalog>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveLocaleRequest {
    pub original_code: Option<String>,
    pub code: String,
    pub name: String,
    pub messages: HashMap<String, String>,
}

const ENGLISH_CATALOG: &str = include_str!("../../src/locales/en.json");

fn write_english_template(path: &Path) -> Result<(), String> {
    let english: serde_json::Map<String, serde_json::Value> = serde_json::from_str(ENGLISH_CATALOG).map_err(|error| format!("The built-in English locale is invalid: {error}"))?;
    let mut template = serde_json::Map::new();
    template.insert("_name".into(), "My language".into());
    template.insert("_code".into(), "".into());
    template.extend(english);
    let text = serde_json::to_string_pretty(&template).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|error| format!("Could not write {}: {error}", path.display()))
}

fn locale_code(value: Option<&serde_json::Value>, path: &Path) -> Result<String, String> {
    let fallback = path.file_stem().and_then(|name| name.to_str()).unwrap_or_default();
    let code = value.and_then(serde_json::Value::as_str).map(str::trim).filter(|code| !code.is_empty()).unwrap_or(fallback).trim();
    if code.is_empty() || code.len() > 32 || !code.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')) {
        return Err("_code must contain 1–32 letters, numbers, '-' or '_'".into());
    }
    if code.eq_ignore_ascii_case("en") {
        return Err("the code 'en' is reserved for JD IDE's built-in English catalog".into());
    }
    Ok(code.to_ascii_lowercase())
}

fn checked_locale_code(value: &str) -> Result<String, String> {
    let synthetic_path = PathBuf::from(format!("{}.json", value.trim()));
    locale_code(None, &synthetic_path)
}

fn placeholders(value: &str) -> HashSet<&str> {
    let mut found = HashSet::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('}') else { break };
        if close > 0 {
            found.insert(&rest[..close]);
        }
        rest = &rest[close + 1..];
    }
    found
}

/// User-authored locale JSON files beside an always-current English template.
pub fn locale_catalogs(user_dir: &Path) -> Result<LocaleCatalogs, String> {
    let directory = user_dir.join("locales");
    std::fs::create_dir_all(&directory).map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
    let template_path = directory.join("_english-template.json");
    write_english_template(&template_path)?;

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map_err(|error| format!("Could not read {}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("json")))
        .filter(|path| !path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with('_')))
        .collect();
    paths.sort_by_key(|path| path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_ascii_lowercase());

    let mut locales = Vec::new();
    let mut errors = Vec::new();
    let mut codes = HashSet::new();
    for path in paths {
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("locale.json");
        let mut read = || -> Result<LocaleCatalog, String> {
            let text = std::fs::read_to_string(&path).map_err(|error| format!("could not read it: {error}"))?;
            let object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).map_err(|error| format!("invalid JSON: {error}"))?;
            let code = locale_code(object.get("_code"), &path)?;
            if !codes.insert(code.clone()) {
                return Err(format!("another locale already uses code '{code}'"));
            }
            let name = object.get("_name").and_then(serde_json::Value::as_str).map(str::trim).filter(|name| !name.is_empty()).unwrap_or(&code).to_string();
            let messages = object.into_iter().filter(|(key, _)| !key.starts_with('_')).filter_map(|(key, value)| value.as_str().map(|value| (key, value.to_string()))).collect();
            Ok(LocaleCatalog { code, name, path: path.display().to_string(), messages })
        };
        match read() {
            Ok(locale) => locales.push(locale),
            Err(error) => errors.push(format!("{file_name}: {error}")),
        }
    }
    Ok(LocaleCatalogs { directory: directory.display().to_string(), template_path: template_path.display().to_string(), locales, errors })
}

/// Creates or updates one user locale. Empty messages are omitted so English remains the fallback.
pub fn save_locale(user_dir: &Path, request: SaveLocaleRequest) -> Result<LocaleCatalogs, String> {
    let code = checked_locale_code(&request.code)?;
    let original_code = request.original_code.as_deref().map(checked_locale_code).transpose()?;
    if original_code.as_deref().is_some_and(|original| original != code) {
        return Err("The locale code cannot be changed after it is created".into());
    }
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("The language name must contain 1–80 characters".into());
    }

    let english: serde_json::Map<String, serde_json::Value> = serde_json::from_str(ENGLISH_CATALOG).map_err(|error| format!("The built-in English locale is invalid: {error}"))?;
    for (key, value) in &request.messages {
        let source = english.get(key).and_then(serde_json::Value::as_str).ok_or_else(|| format!("Unknown translation key '{key}'"))?;
        if !value.trim().is_empty() && placeholders(source) != placeholders(value) {
            return Err(format!("Translation '{key}' must keep the same {{placeholders}} as English"));
        }
    }

    let catalogs = locale_catalogs(user_dir)?;
    let directory = PathBuf::from(&catalogs.directory);
    let target = if let Some(original) = original_code.as_deref() {
        catalogs.locales.iter().find(|catalog| catalog.code == original).map(|catalog| PathBuf::from(&catalog.path)).ok_or_else(|| format!("Locale '{original}' no longer exists; reload locales and try again"))?
    } else {
        if catalogs.locales.iter().any(|catalog| catalog.code == code) {
            return Err(format!("A locale with code '{code}' already exists"));
        }
        let path = directory.join(format!("{code}.json"));
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        path
    };

    let mut output = if original_code.is_some() {
        std::fs::read_to_string(&target)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text).ok())
            .unwrap_or_default()
    } else {
        serde_json::Map::new()
    };
    output.insert("_name".into(), name.into());
    output.insert("_code".into(), code.into());
    for key in english.keys() {
        match request.messages.get(key).filter(|value| !value.trim().is_empty()) {
            Some(value) => { output.insert(key.clone(), value.clone().into()); }
            None => { output.remove(key); }
        }
    }
    let text = serde_json::to_string_pretty(&output).map_err(|error| error.to_string())? + "\n";
    crate::path_data::write_replacing(&target, text.as_bytes())?;
    locale_catalogs(user_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_missing_files() {
        let dir = std::env::temp_dir().join(format!("jdide-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let s = Settings {
            client_dir: Some("E:/Games/ForsakenJD".into()),
            open_on_start: true,
            theme: Theme::Dark,
            language: Some("bg".into()),
            ai_endpoint: Some("https://api.openai.com/v1/responses".into()),
            ai_model: Some("test-model".into()),
            ai_api_key: Some("secret".into()),
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        // Unknown or missing keys fall back to defaults.
        std::fs::write(&path, r#"{ "clientDir": "X", "futureOption": 1 }"#).unwrap();
        assert_eq!(Settings::load(&path), Settings { client_dir: Some("X".into()), open_on_start: false, theme: Theme::System, ..Settings::default() });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_locales_start_from_english_and_allow_partial_translations() {
        let dir = std::env::temp_dir().join(format!("jdide-locales-{}", std::process::id()));
        let locale_dir = dir.join("locales");
        std::fs::create_dir_all(&locale_dir).unwrap();
        std::fs::write(locale_dir.join("test.json"), r#"{ "_name": "Test language", "_code": "tt", "common.save": "Keep" }"#).unwrap();
        std::fs::write(locale_dir.join("fr.json"), r#"{ "_name": "Français", "common.cancel": "Annuler" }"#).unwrap();
        let found = locale_catalogs(&dir).unwrap();
        assert!(Path::new(&found.template_path).is_file());
        assert_eq!(found.locales.len(), 2);
        let translated = found.locales.iter().find(|locale| locale.code == "tt").unwrap();
        assert_eq!(translated.name, "Test language");
        assert_eq!(translated.messages.get("common.save").map(String::as_str), Some("Keep"));
        assert!(translated.messages.get("common.cancel").is_none());
        assert_eq!(found.locales.iter().find(|locale| locale.code == "fr").map(|locale| locale.name.as_str()), Some("Français"));
        let template = std::fs::read_to_string(found.template_path).unwrap();
        assert!(template.contains("\"common.cancel\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_and_updates_a_locale_but_keeps_placeholders() {
        let dir = std::env::temp_dir().join(format!("jdide-locale-editor-{}", std::process::id()));
        let created = save_locale(&dir, SaveLocaleRequest {
            original_code: None,
            code: "DE".into(),
            name: "Deutsch".into(),
            messages: HashMap::from([("common.save".into(), "Speichern".into()), ("save.title".into(), "{file} speichern".into())]),
        }).unwrap();
        let locale = created.locales.iter().find(|locale| locale.code == "de").unwrap();
        assert_eq!(locale.messages.get("common.save").map(String::as_str), Some("Speichern"));

        let updated = save_locale(&dir, SaveLocaleRequest {
            original_code: Some("de".into()),
            code: "de".into(),
            name: "Deutsch (DE)".into(),
            messages: HashMap::from([("common.cancel".into(), "Abbrechen".into())]),
        }).unwrap();
        let locale = updated.locales.iter().find(|locale| locale.code == "de").unwrap();
        assert_eq!(locale.name, "Deutsch (DE)");
        assert!(locale.messages.get("common.save").is_none());
        assert_eq!(locale.messages.get("common.cancel").map(String::as_str), Some("Abbrechen"));

        let error = save_locale(&dir, SaveLocaleRequest {
            original_code: Some("de".into()),
            code: "de".into(),
            name: "Deutsch".into(),
            messages: HashMap::from([("save.title".into(), "Datei speichern".into())]),
        }).unwrap_err();
        assert!(error.contains("{placeholders}"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
