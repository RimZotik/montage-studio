//! Проекты на главной: создание папки проекта по формату (`docs/project-format.md`), открытие
//! существующей, список недавних (SQLite `projects`). Содержимое проекта правит сервис проекта
//! (с 0.3) — здесь только создание каркаса и указатель.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use tauri::State;

use crate::db::Db;
use crate::error::{AppError, AppResult};

pub const PROJECT_VERSION: u32 = 1;
const MEDIA_DIRS: [&str; 5] = ["footage", "voice", "music", "sfx", "images"];
const RULES: [&str; 4] = ["brief", "rules", "style", "script"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    Vertical,
    Horizontal,
    Square,
}

impl Preset {
    fn size(self) -> (u32, u32) {
        match self {
            Self::Vertical => (1080, 1920),
            Self::Horizontal => (1920, 1080),
            Self::Square => (1080, 1080),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub name: String,
    /// Папка, в которой появится папка проекта.
    pub parent: String,
    pub preset: Preset,
    pub fps: u32,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub path: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Unix, секунды.
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub opened_at: i64,
    /// Папка на месте (иначе — переместили или удалили).
    pub exists: bool,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// Имя папки из имени проекта: без символов, запрещенных в Windows.
fn folder_name(name: &str) -> String {
    let cleaned: String = name.trim().chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    cleaned.trim_end_matches(['.', ' ']).to_owned()
}

fn write(path: &Path, text: &str) -> AppResult<()> {
    std::fs::write(path, text).map_err(|e| AppError::new("io", format!("{}: {e}", path.display())))
}

/// Создать каркас проекта на диске; возвращает папку.
pub fn create_on_disk(input: &NewProject) -> AppResult<(PathBuf, (u32, u32))> {
    let name = input.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(AppError::new("invalidName", "project name is empty or too long"));
    }
    if !(1..=240).contains(&input.fps) {
        return Err(AppError::new("invalidFps", format!("fps {}", input.fps)));
    }
    let dir = Path::new(&input.parent).join(folder_name(name));
    if dir.exists() && std::fs::read_dir(&dir)?.next().is_some() {
        return Err(AppError::new("folderNotEmpty", dir.display().to_string()));
    }
    let (width, height) = input.preset.size();
    for sub in MEDIA_DIRS {
        std::fs::create_dir_all(dir.join("media").join(sub))?;
    }
    for sub in ["rules", "smart", "output", ".studio"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    for rule in RULES {
        write(&dir.join("rules").join(format!("{rule}.md")), "")?;
    }
    let project = json!({
        "version": PROJECT_VERSION,
        "name": name,
        "series": null,
        "sequence": { "width": width, "height": height, "fps": input.fps, "durationFrames": 0, "background": "#000000" },
        "libraryRules": [],
    });
    let timeline = json!({ "version": 1, "tracks": [], "clips": [], "markers": [] });
    write(&dir.join("project.json"), &serde_json::to_string_pretty(&project).unwrap_or_default())?;
    write(&dir.join("timeline.json"), &serde_json::to_string_pretty(&timeline).unwrap_or_default())?;
    write(&dir.join(".gitignore"), ".studio/cache/\n.studio/build/\noutput/\n")?;
    Ok((dir, (width, height)))
}

/// Прочитать `project.json` папки: имя и последовательность.
pub fn read_on_disk(dir: &Path) -> AppResult<(String, u32, u32, u32)> {
    let raw = std::fs::read(dir.join("project.json")).map_err(|_| AppError::new("notProject", dir.display().to_string()))?;
    let v: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| AppError::new("badProject", e.to_string()))?;
    let seq = &v["sequence"];
    let num = |k: &str| seq[k].as_u64().and_then(|n| u32::try_from(n).ok());
    match (v["name"].as_str(), num("width"), num("height"), num("fps")) {
        (Some(name), Some(w), Some(h), Some(fps)) => Ok((name.to_owned(), w, h, fps)),
        _ => Err(AppError::new("badProject", "project.json lacks name or sequence")),
    }
}

fn remember(db: &Db, path: &str, name: &str, (w, h, fps): (u32, u32, u32)) -> AppResult<ProjectSummary> {
    let t = now();
    db.with(|c| {
        c.execute(
            "insert into projects (path, name, width, height, fps, created_at, opened_at) values (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             on conflict(path) do update set name = excluded.name, width = excluded.width, height = excluded.height, fps = excluded.fps, opened_at = excluded.opened_at",
            rusqlite::params![path, name, w, h, fps, t],
        )?;
        let created: i64 = c.query_row("select created_at from projects where path = ?1", [path], |r| r.get(0))?;
        Ok(ProjectSummary { path: path.to_owned(), name: name.to_owned(), width: w, height: h, fps, created_at: created, opened_at: t, exists: true })
    })
}

pub fn recent(db: &Db) -> AppResult<Vec<ProjectSummary>> {
    db.with(|c| {
        let mut stmt = c.prepare("select path, name, width, height, fps, created_at, opened_at from projects order by opened_at desc limit 200")?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(0)?;
            Ok(ProjectSummary { exists: Path::new(&path).join("project.json").is_file(), path, name: r.get(1)?, width: r.get(2)?, height: r.get(3)?, fps: r.get(4)?, created_at: r.get(5)?, opened_at: r.get(6)? })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
}

#[tauri::command]
#[specta::specta]
pub fn projects_recent(db: State<'_, Db>) -> AppResult<Vec<ProjectSummary>> {
    recent(&db)
}

#[tauri::command]
#[specta::specta]
pub fn project_create(db: State<'_, Db>, input: NewProject) -> AppResult<ProjectSummary> {
    let (dir, (w, h)) = create_on_disk(&input)?;
    log::info!("project created: {}", dir.display());
    remember(&db, &dir.display().to_string(), input.name.trim(), (w, h, input.fps))
}

#[tauri::command]
#[specta::specta]
pub fn project_open(db: State<'_, Db>, path: String) -> AppResult<ProjectSummary> {
    let (name, w, h, fps) = read_on_disk(Path::new(&path))?;
    log::info!("project opened: {path}");
    remember(&db, &path, &name, (w, h, fps))
}

#[tauri::command]
#[specta::specta]
pub fn project_forget(db: State<'_, Db>, path: String) -> AppResult<()> {
    db.with(|c| Ok(c.execute("delete from projects where path = ?1", [&path]).map(|_| ())?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_open_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::memory().unwrap();
        let input = NewProject { name: "Промо: октябрь?".into(), parent: tmp.path().display().to_string(), preset: Preset::Vertical, fps: 60 };
        let (dir, size) = create_on_disk(&input).unwrap();
        assert_eq!(size, (1080, 1920));
        assert!(dir.ends_with("Промо_ октябрь_"));
        assert!(dir.join("media/voice").is_dir() && dir.join("rules/brief.md").is_file());
        assert_eq!(read_on_disk(&dir).unwrap(), ("Промо: октябрь?".into(), 1080, 1920, 60));
        remember(&db, &dir.display().to_string(), "Промо: октябрь?", (1080, 1920, 60)).unwrap();
        let list = recent(&db).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].exists);
        assert_eq!(create_on_disk(&input).unwrap_err().code, "folderNotEmpty");
    }

    #[test]
    fn rejects_bad_input() {
        let input = NewProject { name: "  ".into(), parent: ".".into(), preset: Preset::Square, fps: 30 };
        assert_eq!(create_on_disk(&input).unwrap_err().code, "invalidName");
        assert_eq!(read_on_disk(Path::new("definitely-missing")).unwrap_err().code, "notProject");
    }
}
