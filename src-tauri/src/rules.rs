//! Правила проекта (`rules/*.md`): их правят пользователь в «Подготовке» и Claude (своими
//! инструментами). Это обычные текстовые файлы вне истории отмены проекта; запись — атомарная.
//! Изменение правил меняет постоянный контекст Claude (перезапуск чата с той же сессией).

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::project::{ProjectState, write_atomic};

pub const NAMES: [&str; 4] = ["brief", "rules", "style", "script"];
const MAX_BYTES: usize = 200_000;

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleFile {
    pub name: String,
    pub text: String,
}

#[tauri::command]
#[specta::specta]
pub fn rules_read(project: State<'_, ProjectState>) -> AppResult<Vec<RuleFile>> {
    let (dir, _) = project.context().ok_or_else(|| AppError::new("noProject", "no project"))?;
    Ok(NAMES.iter().map(|n| RuleFile { name: (*n).to_owned(), text: std::fs::read_to_string(dir.join("rules").join(format!("{n}.md"))).unwrap_or_default() }).collect())
}

#[tauri::command]
#[specta::specta]
pub fn rules_write(project: State<'_, ProjectState>, name: String, text: String) -> AppResult<()> {
    let (dir, _) = project.context().ok_or_else(|| AppError::new("noProject", "no project"))?;
    if !NAMES.contains(&name.as_str()) || text.len() > MAX_BYTES {
        return Err(AppError::new("invalidInput", name));
    }
    let rules = dir.join("rules");
    std::fs::create_dir_all(&rules)?;
    write_atomic(&rules.join(format!("{name}.md")), text.as_bytes())
}
