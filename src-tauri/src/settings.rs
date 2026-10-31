//! Настройки приложения: `settings.json` в папке настроек, с версией формата. Неизвестные и
//! недостающие поля — по умолчанию (`serde(default)`), так старый файл читается новой версией.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    /// `ru` или `en`; пусто — по языку системы (решает интерфейс при первом запуске).
    pub language: String,
    /// Папка новых проектов по умолчанию.
    pub projects_dir: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { version: VERSION, language: String::new(), projects_dir: None }
    }
}

pub struct SettingsState {
    path: PathBuf,
    value: Mutex<Settings>,
}

impl SettingsState {
    pub fn load(path: PathBuf) -> Self {
        let value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default();
        Self { path, value: Mutex::new(Settings { version: VERSION, ..value }) }
    }

    pub fn get(&self) -> Settings {
        self.value.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn save(&self, value: Settings) -> AppResult<Settings> {
        let json = serde_json::to_vec_pretty(&value).map_err(|e| AppError::new("internal", e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)?;
        *self.value.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = value.clone();
        Ok(value)
    }
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(state: State<'_, SettingsState>) -> Settings {
    state.get()
}

#[tauri::command]
#[specta::specta]
pub fn settings_update(state: State<'_, SettingsState>, value: Settings) -> AppResult<Settings> {
    state.save(Settings { version: VERSION, ..value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_file_reads_with_defaults() {
        let s: Settings = serde_json::from_str(r#"{"language":"en","unknown":1}"#).unwrap();
        assert_eq!(s.language, "en");
        assert_eq!(s.projects_dir, None);
    }

    #[test]
    fn save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let state = SettingsState::load(path.clone());
        state.save(Settings { language: "en".into(), ..Settings::default() }).unwrap();
        assert_eq!(SettingsState::load(path).get().language, "en");
    }
}
