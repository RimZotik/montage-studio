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

#[tauri::command]
#[specta::specta]
pub fn errors_count() -> u32 {
    ERRORS.load(std::sync::atomic::Ordering::Relaxed)
}
