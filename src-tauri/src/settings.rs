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
    /// Модель чата Claude: `sonnet` (по умолчанию — быстро и дешевле), `opus`, `haiku`.
    #[serde(default = "sonnet")]
    pub claude_model: String,
    /// Значения интерфейса: ключ `область.имя` → любое значение (`useSetting`, `docs/settings.md`).
    /// Новой настройке не нужен новый код в ядре и новая версия файла.
    #[specta(type = specta_typescript::Any)]
    pub ui: serde_json::Map<String, serde_json::Value>,
}

fn sonnet() -> String {
    "sonnet".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self { version: VERSION, language: String::new(), projects_dir: None, claude_model: sonnet(), ui: serde_json::Map::new() }
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

    /// Записать значения интерфейса; `null` — вернуть к умолчанию (ключ убирается).
    pub fn set_ui(&self, values: serde_json::Map<String, serde_json::Value>) -> AppResult<()> {
        let mut next = self.get();
        for (key, value) in values {
            if key.len() > 120 || value.to_string().len() > 64 * 1024 {
                return Err(AppError::new("invalidSetting", key));
            }
            if value.is_null() {
                next.ui.remove(&key);
            } else {
                next.ui.insert(key, value);
            }
        }
        self.save(next).map(|_| ())
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
    // Значения интерфейса меняются только своей командой: окно могло держать старую копию.
    state.save(Settings { version: VERSION, ui: state.get().ui, ..value })
}

/// Изменить значения интерфейса пачкой, остальное не трогается.
#[tauri::command]
#[specta::specta]
pub fn settings_set_ui(state: State<'_, SettingsState>, values: crate::presets::Json) -> AppResult<()> {
    state.set_ui(values.0.as_object().cloned().unwrap_or_default())
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
    fn ui_values_keep_between_runs_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let state = SettingsState::load(path.clone());
        state.set_ui(serde_json::json!({ "monitor.safeZone": false, "layout.edit": { "media": 300 } }).as_object().unwrap().clone()).unwrap();
        let again = SettingsState::load(path.clone());
        assert_eq!(again.get().ui["monitor.safeZone"], serde_json::json!(false));
        again.set_ui(serde_json::json!({ "monitor.safeZone": null }).as_object().unwrap().clone()).unwrap();
        let last = SettingsState::load(path).get();
        assert!(!last.ui.contains_key("monitor.safeZone"), "null возвращает к умолчанию");
        assert!(last.ui.contains_key("layout.edit"), "другие ключи на месте");
        assert!(state.set_ui(serde_json::json!({ "big": "x".repeat(70_000) }).as_object().unwrap().clone()).is_err());
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
