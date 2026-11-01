//! Ошибка команд для интерфейса: стабильный код (по нему выбирается перевод) и текст для журнала.

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    /// Каждая ошибка, уходящая в интерфейс, сразу пишется в журнал.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        let error = Self { code: code.into(), message: message.into() };
        ERRORS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut recent) = RECENT.lock() {
            let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
            recent.push(ErrorEntry { at, code: error.code.clone(), message: error.message.clone() });
            let excess = recent.len().saturating_sub(100);
            recent.drain(..excess);
        }
        log::warn!("error {}: {}", error.code, error.message);
        error
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new("database", error.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::new("io", error.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Сколько ошибок записано в журнал с запуска (значок ошибок в строке состояния).
pub static ERRORS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorEntry {
    #[specta(type = specta_typescript::Number)]
    pub at: i64,
    pub code: String,
    pub message: String,
}

/// Последние ошибки (до 100) для окошка ошибок.
static RECENT: std::sync::Mutex<Vec<ErrorEntry>> = std::sync::Mutex::new(Vec::new());

#[tauri::command]
#[specta::specta]
pub fn errors_count() -> u32 {
    ERRORS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Последние ошибки, новые первыми.
#[tauri::command]
#[specta::specta]
pub fn errors_recent() -> Vec<ErrorEntry> {
    RECENT.lock().map(|r| r.iter().rev().cloned().collect()).unwrap_or_default()
}

/// Ошибка интерфейса (страница упала, необработанное исключение) — в журнал и окошко ошибок.
#[tauri::command]
#[specta::specta]
pub fn ui_log(level: String, message: String) {
    if level == "error" {
        let _ = AppError::new("ui", message);
    } else {
        log::info!(target: "ui", "{message}");
    }
}

/// «Пропустить»: очистить окошко ошибок (журнал не трогается).
#[tauri::command]
#[specta::specta]
pub fn errors_clear() {
    ERRORS.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut r) = RECENT.lock() {
        r.clear();
    }
}
