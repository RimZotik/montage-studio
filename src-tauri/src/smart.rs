//! Умные клипы: сцены кодом в папке `smart/` проекта (компоненты на React и Remotion). Монитор и рендер
//! исполняют один и тот же исходник, поэтому картинка совпадает. Код выполняется с правами программы,
//! поэтому проект с кодом запускается только после явного доверия; созданный здесь — доверен сразу.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::project::ProjectState;
use crate::settings::SettingsState;

const EXTENSIONS: [&str; 5] = ["tsx", "ts", "jsx", "js", "json"];
const MAX_FILE: u64 = 1024 * 1024;
const MAX_TOTAL: u64 = 16 * 1024 * 1024;

/// Исходники `smart/`: путь от проекта через `/` → текст.
pub fn sources(project: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut total = 0;
    let mut stack = vec![project.join("smart")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let known = path.extension().and_then(|x| x.to_str()).is_some_and(|x| EXTENSIONS.contains(&x));
            let size = e.metadata().map_or(u64::MAX, |m| m.len());
            if !known || size > MAX_FILE || total + size > MAX_TOTAL {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            total += size;
            let rel = path.strip_prefix(project).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            out.insert(rel, text);
        }
    }
    out
}

/// Исходники для запуска: пусто, пока проекту не доверяют.
pub fn runnable(app: &AppHandle, project: &Path) -> BTreeMap<String, String> {
    if is_trusted(app, project) { sources(project) } else { BTreeMap::new() }
}

pub fn is_trusted(app: &AppHandle, project: &Path) -> bool {
    let me = crate::projects::canonical(&project.display().to_string()).to_lowercase();
    app.try_state::<SettingsState>().is_some_and(|s| s.get().trusted_code.iter().any(|p| p.to_lowercase() == me))
}

pub fn trust(state: &SettingsState, project: &Path) -> AppResult<()> {
    state.trust_code(crate::projects::canonical(&project.display().to_string()))
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SmartState {
    pub trusted: bool,
    /// В проекте есть код, который без доверия не запускается.
    pub has_code: bool,
    pub sources: BTreeMap<String, String>,
}

/// Код проекта поменялся (монитор перечитывает).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct SmartChanged;

#[tauri::command]
#[specta::specta]
pub fn smart_state(app: AppHandle, project: State<'_, ProjectState>) -> SmartState {
    let Some((dir, _)) = project.context() else { return SmartState { trusted: false, has_code: false, sources: BTreeMap::new() } };
    let all = sources(&dir);
    let trusted = is_trusted(&app, &dir);
    SmartState { trusted, has_code: !all.is_empty(), sources: if trusted { all } else { BTreeMap::new() } }
}

/// Пользователь разрешил запускать код этого проекта.
#[tauri::command]
#[specta::specta]
pub fn smart_trust(app: AppHandle, project: State<'_, ProjectState>, settings: State<'_, SettingsState>) -> AppResult<()> {
    let (dir, _) = project.context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
    trust(&settings, &dir)?;
    let _ = tauri_specta::Event::emit(&SmartChanged, &app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_code_with_project_paths() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("smart/lib")).unwrap();
        std::fs::write(t.path().join("smart/Card.tsx"), "export default 1").unwrap();
        std::fs::write(t.path().join("smart/lib/data.json"), "{}").unwrap();
        std::fs::write(t.path().join("smart/run.exe"), "x").unwrap();
        let got = sources(t.path());
        assert_eq!(got.keys().map(String::as_str).collect::<Vec<_>>(), ["smart/Card.tsx", "smart/lib/data.json"]);
    }
}
