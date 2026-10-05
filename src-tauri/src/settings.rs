//! User settings, stored as `settings.json` in the app config folder.

use std::path::Path;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_missing_files() {
        let dir = std::env::temp_dir().join(format!("jdide-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(Settings::load(&path), Settings::default());
        let s = Settings { client_dir: Some("E:/Games/ForsakenJD".into()), open_on_start: true, theme: Theme::Dark };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        // Unknown or missing keys fall back to defaults.
        std::fs::write(&path, r#"{ "clientDir": "X", "futureOption": 1 }"#).unwrap();
        assert_eq!(Settings::load(&path), Settings { client_dir: Some("X".into()), open_on_start: false, theme: Theme::System });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
