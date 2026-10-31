//! Ядро Montage Studio: команды для интерфейса, SQLite, проверки системы, проекты, лимиты Claude.
//! Устройство — `docs/architecture.md`.

mod checks;
mod claude_limits;
mod db;
mod error;
mod ipc;
mod projects;
mod settings;

use tauri::Manager;

pub fn run() {
    let ipc = ipc::builder();
    #[cfg(debug_assertions)]
    ipc::export_bindings(&ipc);

    tauri::Builder::default()
        // Первым: второй запуск поднимает уже открытое окно, а не открывает новое.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir { file_name: Some("studio".into()) }))
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout))
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(ipc.invoke_handler())
        .setup(move |app| {
            ipc.mount_events(app);
            let config = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config)?;
            app.manage(settings::SettingsState::load(config.join("settings.json")));
            app.manage(db::Db::open(&config.join("studio.db")).map_err(|e| e.message)?);
            log::info!("Montage Studio {} started; config={}", app.package_info().version, config.display());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Montage Studio");
}
