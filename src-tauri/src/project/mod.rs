//! Сервис открытого проекта (`docs/project-format.md`): единственный, кто пишет `project.json` и
//! `timeline.json`. Каждая правка — проверка данных, атомарная запись, шаг истории (отмена и
//! повтор, `history.rs`) и событие `ProjectChanged` в интерфейс. Интерфейс и (позже) Claude
//! меняют проект только через команды сервиса.

mod history;
pub mod model;
pub mod timeline;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use history::History;
use model::{ProjectFile, Sequence, Snapshot};

/// Что видит интерфейс об открытом проекте.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub path: String,
    pub project: ProjectFile,
    pub tracks: Vec<timeline::Track>,
    pub clips: Vec<timeline::Clip>,
    pub markers: Vec<timeline::Marker>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Что отменит «Отменить» / вернет «Повторить».
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct ProjectChanged(pub Option<ProjectView>);

struct Open {
    dir: PathBuf,
    doc: Snapshot,
    history: History,
}

impl Open {
    fn view(&self) -> ProjectView {
        ProjectView {
            path: self.dir.display().to_string(),
            project: self.doc.project.clone(),
            tracks: self.doc.timeline.tracks.clone(),
            clips: self.doc.timeline.clips.clone(),
            markers: self.doc.timeline.markers.clone(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            undo_label: self.history.undo_label().map(str::to_owned),
            redo_label: self.history.redo_label().map(str::to_owned),
        }
    }
}

#[derive(Default)]
pub struct ProjectState(Mutex<Option<Open>>);

/// Записать файл атомарно: во временный рядом, затем переименовать поверх.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        AppError::from(e)
    })
}

fn to_json<T: Serialize>(value: &T) -> AppResult<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| AppError::new("internal", e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Записать на диск то, что изменилось между `before` и `after`.
fn persist(dir: &Path, before: Option<&Snapshot>, after: &Snapshot) -> AppResult<()> {
    if before.is_none_or(|b| b.project != after.project) {
        write_atomic(&dir.join("project.json"), &to_json(&after.project)?)?;
    }
    if before.is_none_or(|b| b.timeline != after.timeline) {
        write_atomic(&dir.join("timeline.json"), &to_json(&after.timeline)?)?;
    }
    Ok(())
}

/// Прочитать проект с диска (с миграцией старых версий формата: старый файл — в
/// `.studio/history/` перед записью новой версии).
pub fn load(dir: &Path) -> AppResult<Snapshot> {
    let read = |name: &str| -> AppResult<serde_json::Value> {
        let raw = std::fs::read(dir.join(name)).map_err(|_| AppError::new(if name == "project.json" { "notProject" } else { "badProject" }, format!("{name} missing")))?;
        serde_json::from_slice(&raw).map_err(|e| AppError::new("badProject", format!("{name}: {e}")))
    };
    let (project_raw, timeline_raw) = (read("project.json")?, read("timeline.json")?);
    let (project, migrated_p) = model::migrate_project(project_raw.clone())?;
    let (timeline, migrated_t) = model::migrate_timeline(timeline_raw.clone())?;
    let snapshot = Snapshot { project, timeline };
    model::validate(&snapshot)?;
    if migrated_p || migrated_t {
        let backup = dir.join(".studio").join("history");
        std::fs::create_dir_all(&backup)?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        write_atomic(&backup.join(format!("before-migration-{stamp}.json")), &to_json(&serde_json::json!({ "project": project_raw, "timeline": timeline_raw }))?)?;
        persist(dir, None, &snapshot)?;
        log::info!("project migrated: {}", dir.display());
    }
    Ok(snapshot)
}

fn emit(app: &AppHandle, view: Option<ProjectView>) {
    let _ = ProjectChanged(view).emit(app);
}

impl ProjectState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Open>> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Правка открытого проекта: `change` меняет копию, затем проверка, запись, шаг истории.
    pub fn edit(&self, label: &str, change: impl FnOnce(&mut Snapshot) -> AppResult<()>) -> AppResult<ProjectView> {
        let mut guard = self.lock();
        let open = guard.as_mut().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
        let mut next = open.doc.clone();
        change(&mut next)?;
        if next == open.doc {
            return Ok(open.view());
        }
        model::validate(&next)?;
        persist(&open.dir, Some(&open.doc), &next)?;
        let before = std::mem::replace(&mut open.doc, next);
        open.history.push(label, before, open.doc.clone());
        Ok(open.view())
    }

    /// Открыть проект напрямую (тесты).
    #[cfg(test)]
    pub fn open_for_test(&self, dir: PathBuf, doc: Snapshot) {
        *self.lock() = Some(Open { dir, doc, history: History::default() });
    }

    /// Подпись последнего шага истории.
    #[cfg(test)]
    pub fn undo_label(&self) -> Option<String> {
        self.lock().as_ref().and_then(|o| o.history.undo_label().map(str::to_owned))
    }

    /// Папка и состояние открытого проекта (для чата и других частей).
    pub fn context(&self) -> Option<(PathBuf, Snapshot)> {
        self.lock().as_ref().map(|o| (o.dir.clone(), o.doc.clone()))
    }

    fn step(&self, forward: bool) -> AppResult<ProjectView> {
        let mut guard = self.lock();
        let open = guard.as_mut().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
        let target = if forward { open.history.redo() } else { open.history.undo() };
        if let Some(target) = target {
            persist(&open.dir, Some(&open.doc), &target)?;
            open.doc = target;
        }
        Ok(open.view())
    }
}

#[tauri::command]
#[specta::specta]
pub fn project_load(app: AppHandle, path: String) -> AppResult<ProjectView> {
    open(&app, &path)
}

/// Открыть проект (из интерфейса или инструментов разработчика).
pub fn open(app: &AppHandle, path: &str) -> AppResult<ProjectView> {
    let state = app.state::<ProjectState>();
    let db = app.state::<Db>();
    let path = path.to_owned();
    let dir = PathBuf::from(&path);
    let doc = load(&dir)?;
    if let Some(chat) = app.try_state::<crate::chat::ChatState>() {
        crate::chat::shutdown(&chat);
    }
    let seq = &doc.project.sequence;
    crate::projects::remember(&db, &path, &doc.project.name, (seq.width, seq.height, seq.fps))?;
    let view = {
        let mut guard = state.lock();
        *guard = Some(Open { dir, doc, history: History::default() });
        guard.as_ref().map(Open::view)
    };
    log::info!("project loaded: {path}");
    emit(app, view.clone());
    view.ok_or_else(|| AppError::new("internal", "project vanished"))
}

#[tauri::command]
#[specta::specta]
pub fn project_close(app: AppHandle, state: State<'_, ProjectState>) {
    if let Some(chat) = tauri::Manager::try_state::<crate::chat::ChatState>(&app) {
        crate::chat::shutdown(&chat);
    }
    *state.lock() = None;
    emit(&app, None);
}

#[tauri::command]
#[specta::specta]
pub fn project_current(state: State<'_, ProjectState>) -> Option<ProjectView> {
    state.lock().as_ref().map(Open::view)
}

/// Настройки последовательности (с отменой).
#[tauri::command]
#[specta::specta]
pub fn project_set_sequence(app: AppHandle, state: State<'_, ProjectState>, sequence: Sequence) -> AppResult<ProjectView> {
    let view = state.edit("sequence", |doc| {
        doc.project.sequence = sequence;
        Ok(())
    })?;
    emit(&app, Some(view.clone()));
    Ok(view)
}

/// Переименовать проект (имя в `project.json`; папка остается).
#[tauri::command]
#[specta::specta]
pub fn project_rename(app: AppHandle, state: State<'_, ProjectState>, db: State<'_, Db>, name: String) -> AppResult<ProjectView> {
    let view = state.edit("rename", |doc| {
        doc.project.name = name.trim().to_owned();
        Ok(())
    })?;
    let seq = &view.project.sequence;
    crate::projects::remember(&db, &view.path, &view.project.name, (seq.width, seq.height, seq.fps))?;
    emit(&app, Some(view.clone()));
    Ok(view)
}

/// Правка таймлайна пакетом операций (`timeline::Op`): проверяется и применяется целиком, один
/// шаг истории с подписью `label`. Возвращает ID созданных клипов.
#[tauri::command]
#[specta::specta]
pub fn timeline_apply(app: AppHandle, state: State<'_, ProjectState>, ops: Vec<timeline::Op>, label: String) -> AppResult<Vec<String>> {
    let mut created = Vec::new();
    let view = state.edit(&label, |doc| {
        let t = &mut doc.timeline;
        let mut e = timeline::Edit { tracks: &mut t.tracks, clips: &mut t.clips, markers: &mut t.markers };
        for op in &ops {
            created.extend(e.apply(op)?);
        }
        Ok(())
    })?;
    emit(&app, Some(view));
    Ok(created)
}

/// Отменить весь последний ход Claude (все шаги с его подписью `claude#N` подряд).
#[tauri::command]
#[specta::specta]
pub fn project_undo_claude(app: AppHandle, state: State<'_, ProjectState>) -> AppResult<ProjectView> {
    let label = state.lock().as_ref().and_then(|o| o.history.undo_label().map(str::to_owned));
    let Some(label) = label.filter(|l| l.starts_with("claude#")) else {
        return Err(AppError::new("nothingToUndo", "last change is not by Claude"));
    };
    let mut view = None;
    while state.lock().as_ref().and_then(|o| o.history.undo_label().map(str::to_owned)).as_deref() == Some(label.as_str()) {
        view = Some(state.step(false)?);
    }
    let view = view.ok_or_else(|| AppError::new("nothingToUndo", "nothing"))?;
    emit(&app, Some(view.clone()));
    Ok(view)
}

#[tauri::command]
#[specta::specta]
pub fn project_undo(app: AppHandle, state: State<'_, ProjectState>) -> AppResult<ProjectView> {
    let view = state.step(false)?;
    emit(&app, Some(view.clone()));
    Ok(view)
}

#[tauri::command]
#[specta::specta]
pub fn project_redo(app: AppHandle, state: State<'_, ProjectState>) -> AppResult<ProjectView> {
    let view = state.step(true)?;
    emit(&app, Some(view.clone()));
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{NewProject, Preset, create_on_disk};

    fn open_tmp() -> (tempfile::TempDir, ProjectState) {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, _) = create_on_disk(&NewProject { name: "T".into(), parent: tmp.path().display().to_string(), preset: Preset::Vertical, fps: 60 }).unwrap();
        let doc = load(&dir).unwrap();
        let state = ProjectState::default();
        *state.lock() = Some(Open { dir, doc, history: History::default() });
        (tmp, state)
    }

    fn disk_fps(state: &ProjectState) -> u32 {
        let dir = state.lock().as_ref().unwrap().dir.clone();
        load(&dir).unwrap().project.sequence.fps
    }

    #[test]
    fn edit_undo_redo_persist() {
        let (_tmp, state) = open_tmp();
        let v = state.edit("sequence", |d| {
            d.project.sequence.fps = 30;
            Ok(())
        });
        assert!(v.unwrap().can_undo);
        assert_eq!(disk_fps(&state), 30);
        let v = state.step(false).unwrap();
        assert_eq!((v.project.sequence.fps, v.can_undo, v.can_redo), (60, false, true));
        assert_eq!(disk_fps(&state), 60, "отмена записана на диск");
        assert_eq!(state.step(true).unwrap().project.sequence.fps, 30);
        assert_eq!(disk_fps(&state), 30);
    }

    #[test]
    fn invalid_edit_is_rejected_whole() {
        let (_tmp, state) = open_tmp();
        let err = state.edit("sequence", |d| {
            d.project.sequence.width = 3;
            Ok(())
        });
        assert_eq!(err.unwrap_err().code, "invalidSequence");
        let v = state.lock().as_ref().unwrap().view();
        assert_eq!((v.project.sequence.width, v.can_undo), (1080, false));
    }

    #[test]
    fn noop_edit_is_not_a_step() {
        let (_tmp, state) = open_tmp();
        let v = state.edit("nothing", |_| Ok(())).unwrap();
        assert!(!v.can_undo);
    }
}
