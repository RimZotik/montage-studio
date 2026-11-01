//! Ядро Montage Studio: команды для интерфейса, SQLite, проверки системы, проекты, лимиты Claude.
//! Устройство — `docs/architecture.md`.

mod chat;
mod checks;
mod claude_limits;
mod components;
mod db;
mod error;
mod ipc;
mod mcp;
mod media;
mod project;
mod projects;
mod rules;
mod settings;
mod updates;

use std::sync::Arc;

use tauri::Manager;

/// Журнал по группам (`docs/architecture.md`): `app` — все, `errors` — предупреждения и ошибки,
/// `claude` — чат и лимиты, `media` — медиа и проекты, `system` — проверки, компоненты, обновления.
/// Открытым текстом (код открытый, личного в журнале нет), файлы — в папке журнала приложения.
fn logging() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_log::{Target, TargetKind};
    let file = |name: &str| Target::new(TargetKind::LogDir { file_name: Some(name.into()) });
    let group = |name: &'static str, parts: &'static [&'static str]| file(name).filter(move |m| parts.iter().any(|p| m.target().contains(p)));
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .level(log::LevelFilter::Info)
        .max_file_size(5_000_000)
        .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
        .target(Target::new(TargetKind::Stdout))
        .target(file("app"))
        .target(file("errors").filter(|m| m.level() <= log::Level::Warn))
        .target(group("claude", &["::chat", "claude_limits"]))
        .target(group("media", &["::media", "::project", "::projects", "::rules"]))
        .target(group("system", &["::checks", "::components", "::updates"]))
        .build()
}

/// Показать главное окно: обычно — развернутым и в фокусе; `MONTAGE_BACKGROUND=1` (проверки
/// разработчика) — за пределами экрана и без фокуса, чтобы не мешать пользователю.
fn show_main(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if std::env::var("MONTAGE_BACKGROUND").is_ok_and(|v| v == "1") {
        let _ = w.set_size(tauri::PhysicalSize::new(1600, 900));
        let _ = w.set_position(tauri::PhysicalPosition::new(-4000, 0));
        #[cfg(windows)]
        if let Ok(h) = w.hwnd() {
            use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};
            // Показать, не забирая фокус.
            unsafe {
                let _ = ShowWindow(windows::Win32::Foundation::HWND(h.0), SW_SHOWNOACTIVATE);
            }
        }
        log::info!("main window shown in background mode");
        return;
    }
    let _ = w.maximize();
    let _ = w.show();
    let _ = w.set_focus();
}

pub fn run() {
    // Паника в любом потоке — в журнал (иначе в выпуске без консоли ее не увидеть).
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    let ipc = ipc::builder();
    #[cfg(debug_assertions)]
    ipc::export_bindings(&ipc);

    tauri::Builder::default()
        // Первым: второй запуск поднимает уже открытое окно, а не открывает новое.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Второй запуск — поднять открытое окно (и вернуть на экран, если оно было в фоне).
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.maximize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(logging())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(ipc.invoke_handler())
        .manage(components::Installs::default())
        .manage(project::ProjectState::default())
        .manage(chat::ChatState::default())
        .manage(media::MediaJobs::default())
        .manage(mcp::McpState::default())
        .manage(Arc::new(updates::UpdateState::default()))
        .setup(move |app| {
            ipc.mount_events(app);
            // TLS для загрузок — на ring (один раз на процесс).
            let _ = rustls::crypto::ring::default_provider().install_default();
            let config = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config)?;
            app.manage(settings::SettingsState::load(config.join("settings.json")));
            app.manage(db::Db::open(&config.join("studio.db")).map_err(|e| e.message)?);
            log::info!("Montage Studio {} started; config={}", app.package_info().version, config.display());
            show_main(app.handle());
            updates::start(app.handle());
            // MCP-сервер студии для Claude (адрес и ключ — в mcp.json чата проекта).
            let info = mcp::start_app(app.handle());
            // Сборка разработки (или MONTAGE_MCP_EXPOSE=1): адрес и ключ — в mcp-dev.json, чтобы
            // внешний Claude мог проверять студию теми же инструментами.
            if let Some(i) = &info
                && (cfg!(debug_assertions) || std::env::var("MONTAGE_MCP_EXPOSE").is_ok_and(|v| v == "1"))
            {
                let _ = std::fs::write(config.join("mcp-dev.json"), serde_json::json!({ "url": i.url, "token": i.dev_token }).to_string());
            }
            *app.state::<mcp::McpState>().info.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = info;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Montage Studio");
}
