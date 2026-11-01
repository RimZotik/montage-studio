//! Команды для интерфейса (tauri-specta): список и выгрузка типов в `src/shared/ipc/bindings.ts`.

use tauri_specta::{Builder, collect_commands, collect_events};

use crate::{chat, error, checks, claude_limits, components, mcp, media, project, projects, rules, settings, updates};

#[cfg(any(debug_assertions, test))]
pub const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/shared/ipc/bindings.ts");

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        settings::settings_get,
        settings::settings_update,
        checks::system_check,
        projects::projects_recent,
        projects::project_create,
        projects::project_forget,
        claude_limits::claude_limits_last,
        claude_limits::claude_limits_refresh,
        components::component_install,
        components::component_cancel,
        updates::update_status,
        updates::update_check_now,
        updates::update_install,
        project::project_load,
        project::project_close,
        project::project_current,
        project::project_set_sequence,
        project::project_rename,
        project::project_undo,
        project::project_redo,
        project::timeline_apply,
        project::project_undo_claude,
        mcp::ui_state_report,
        error::errors_count,
        error::errors_recent,
        error::errors_clear,
        error::ui_log,
        chat::chat_send,
        chat::chat_stop,
        chat::chat_new_session,
        chat::chat_history,
        chat::chat_busy,
        media::media_list,
        media::media_import,
        media::media_move,
        rules::rules_read,
        rules::rules_write,
    ])
    .events(collect_events![components::ComponentProgress, updates::UpdateChanged, project::ProjectChanged, chat::ChatEvent, claude_limits::LimitsChanged, media::MediaChanged, mcp::UiCommand])
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
