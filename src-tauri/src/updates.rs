//! Обновления приложения (обновлятор Tauri, подпись проверяется открытым ключом из
//! `tauri.conf.json`; `docs/updates.md`).
//!
//! Политика: проверка через `FIRST_CHECK` после запуска и затем раз в `CHECK_EVERY`; нашлось —
//! установщик тихо скачивается в фоне, во второй строке появляется «Обновить до X» — пользователь
//! сам выбирает момент (может идти рендер). Установка без окон (`installMode: quiet`), после нее
//! приложение запускается уже новым. В разработке (`tauri dev`) выключено — иначе отладочная
//! сборка поставила бы поверх себя выпуск.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;

const FIRST_CHECK: Duration = Duration::from_secs(8);
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpdatePhase {
    Idle,
    Downloading,
    /// Скачано — ждет команды «Обновить».
    Ready,
    Installing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Версия, которую можно поставить.
    pub version: Option<String>,
    pub notes: Option<String>,
    pub phase: UpdatePhase,
    /// 0–1.
    pub progress: f32,
}

#[derive(Debug, Clone, Serialize, Type, Event)]
pub struct UpdateChanged(pub UpdateStatus);

pub struct UpdateState {
    status: std::sync::Mutex<UpdateStatus>,
    /// Найденное обновление и скачанный установщик.
    ready: tokio::sync::Mutex<Option<(Update, Vec<u8>)>>,
    busy: AtomicBool,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self { status: std::sync::Mutex::new(UpdateStatus { version: None, notes: None, phase: UpdatePhase::Idle, progress: 0.0 }), ready: tokio::sync::Mutex::new(None), busy: AtomicBool::new(false) }
    }
}

fn set_status(app: &AppHandle, change: impl FnOnce(&mut UpdateStatus)) {
    let Some(state) = app.try_state::<Arc<UpdateState>>() else { return };
    let mut status = state.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let before = status.clone();
    change(&mut status);
    if *status != before {
        let _ = UpdateChanged(status.clone()).emit(app);
    }
}

pub fn enabled() -> bool {
    !cfg!(debug_assertions)
}

async fn check(app: &AppHandle) -> Result<Option<Update>, String> {
    let updater = app.updater_builder().timeout(CHECK_TIMEOUT).build().map_err(|e| e.to_string())?;
    let mut update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(u) = update.as_mut() {
        u.timeout = Some(DOWNLOAD_TIMEOUT);
    }
    Ok(update)
}

/// Проверить и, если есть новое, тихо скачать.
async fn check_and_download(app: &AppHandle) {
    let Some(state) = app.try_state::<Arc<UpdateState>>().map(|s| Arc::clone(&s)) else { return };
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let result = async {
        let Some(update) = check(app).await? else { return Ok::<_, String>(()) };
        let already = state.ready.lock().await.as_ref().is_some_and(|(u, _)| u.version == update.version);
        if already {
            return Ok(());
        }
        log::info!("update {} found, downloading", update.version);
        set_status(app, |s| {
            s.version = Some(update.version.clone());
            s.notes = update.body.clone();
            s.phase = UpdatePhase::Downloading;
            s.progress = 0.0;
        });
        let mut received: u64 = 0;
        let mut last = -1.0f32;
        let bytes = update
            .download(
                |chunk, total| {
                    received = received.saturating_add(chunk as u64);
                    let p = total.filter(|t| *t > 0).map_or(0.0, |t| (received as f64 / t as f64).min(1.0) as f32);
                    if p - last >= 0.02 || p >= 1.0 {
                        last = p;
                        set_status(app, |s| s.progress = p);
                    }
                },
                || {},
            )
            .await
            .map_err(|e| e.to_string())?;
        log::info!("update {}: downloaded {} bytes", update.version, bytes.len());
        *state.ready.lock().await = Some((update, bytes));
        set_status(app, |s| {
            s.phase = UpdatePhase::Ready;
            s.progress = 1.0;
        });
        Ok(())
    }
    .await;
    if let Err(error) = result {
        log::info!("update check failed: {error}");
        set_status(app, |s| {
            if s.phase == UpdatePhase::Downloading {
                s.phase = UpdatePhase::Failed;
            }
        });
    }
    state.busy.store(false, Ordering::SeqCst);
}

/// Фоновые проверки: первая вскоре после запуска, дальше раз в `CHECK_EVERY`.
pub fn start(app: &AppHandle) {
    if !enabled() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check_and_download(&app).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[tauri::command]
#[specta::specta]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    app.try_state::<Arc<UpdateState>>().map_or(UpdateStatus { version: None, notes: None, phase: UpdatePhase::Idle, progress: 0.0 }, |s| s.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone())
}

/// Проверить сейчас (меню «Справка → Проверить обновления»).
#[tauri::command]
#[specta::specta]
pub async fn update_check_now(app: AppHandle) {
    if enabled() {
        check_and_download(&app).await;
    }
}

/// Поставить скачанное обновление: тихая установка, приложение перезапустится новым.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: AppHandle) -> Result<(), crate::error::AppError> {
    let Some(state) = app.try_state::<Arc<UpdateState>>().map(|s| Arc::clone(&s)) else { return Ok(()) };
    let Some((update, bytes)) = state.ready.lock().await.take() else { return Ok(()) };
    set_status(&app, |s| s.phase = UpdatePhase::Installing);
    log::info!("update {}: installing", update.version);
    log::logger().flush();
    update.install(bytes).map_err(|e| {
        set_status(&app, |s| s.phase = UpdatePhase::Failed);
        crate::error::AppError::new("updateInstall", e.to_string())
    })
}
