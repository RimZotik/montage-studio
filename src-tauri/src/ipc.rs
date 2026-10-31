//! Команды для интерфейса (tauri-specta): список и выгрузка типов в `src/shared/ipc/bindings.ts`.

use tauri_specta::{Builder, collect_commands};

use crate::{checks, claude_limits, projects, settings};

#[cfg(any(debug_assertions, test))]
pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/shared/ipc/bindings.ts");

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        settings::settings_get,
        settings::settings_update,
        checks::system_check,
        projects::projects_recent,
        projects::project_create,
        projects::project_open,
        projects::project_forget,
        claude_limits::claude_limits_last,
        claude_limits::claude_limits_refresh,
    ])
}

#[cfg(any(debug_assertions, test))]
fn typescript() -> specta_typescript::Typescript {
    specta_typescript::Typescript::default().header("// Сгенерировано из Rust (src-tauri/src/ipc.rs). Не редактировать вручную.\n")
}

#[cfg(debug_assertions)]
pub fn export_bindings(builder: &Builder<tauri::Wry>) {
    if let Err(error) = builder.export(typescript(), BINDINGS_PATH) {
        log::warn!("failed to export TypeScript bindings: {error}");
    }
}

#[cfg(test)]
mod tests {
    /// `cargo test` обновляет bindings.ts без запуска приложения.
    #[test]
    fn export_typescript_bindings() {
        super::builder().export(super::typescript(), super::BINDINGS_PATH).expect("bindings export");
    }
}
