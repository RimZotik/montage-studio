//! Сервис кэша: единственное место, которое знает, где лежит пересоздаваемое. Все в кэше можно
//! удалить без потери работы: миниатюры, копии и расшифровка строятся заново (`docs/storage.md`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::project::ProjectState;

/// Кэш проекта: `.studio/cache/<вид>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Thumbs,
    Proxy,
    Speech,
    Frames,
}

impl Kind {
    const ALL: [Self; 4] = [Self::Thumbs, Self::Proxy, Self::Speech, Self::Frames];

    fn id(self) -> &'static str {
        match self {
            Self::Thumbs => "thumbs",
            Self::Proxy => "proxy",
            Self::Speech => "speech",
            Self::Frames => "frames",
        }
    }
}

pub fn root(project: &Path) -> PathBuf {
    project.join(".studio").join("cache")
}

pub fn dir(project: &Path, kind: Kind) -> PathBuf {
    root(project).join(kind.id())
}

/// Кэш программы (не привязан к проекту).
pub fn app_dir<R: tauri::Runtime>(app: &AppHandle<R>, name: &str) -> AppResult<PathBuf> {
    Ok(app.path().app_cache_dir().map_err(|e| AppError::new("internal", e.to_string()))?.join(name))
}

fn measure(path: &Path) -> (u64, u32) {
    let (mut bytes, mut files) = (0, 0);
    for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
        match e.metadata() {
            Ok(m) if m.is_dir() => {
                let (b, f) = measure(&e.path());
                bytes += b;
                files += f;
            }
            Ok(m) => {
                bytes += m.len();
                files += 1;
            }
            Err(_) => {}
        }
    }
    (bytes, files)
}

/// Кадры для Claude нужны только в момент проверки: старые убираются при открытии проекта.
pub fn trim_on_open(project: &Path) {
    let limit = std::time::Duration::from_secs(7 * 24 * 3600);
    for e in std::fs::read_dir(dir(project, Kind::Frames)).into_iter().flatten().flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub id: String,
    /// `project` или `app`.
    pub scope: String,
    #[specta(type = specta_typescript::Number)]
    pub bytes: u64,
    pub files: u32,
}

/// Сколько занимает каждый кэш.
pub fn usage(app: &AppHandle) -> AppResult<Vec<CacheEntry>> {
    let mut out = Vec::new();
    if let Some((project, _)) = app.state::<ProjectState>().context() {
        for k in Kind::ALL {
            let (bytes, files) = measure(&dir(&project, k));
            out.push(CacheEntry { id: k.id().into(), scope: "project".into(), bytes, files });
        }
    }
    let (bytes, files) = measure(&app_dir(app, "sounds")?);
    out.push(CacheEntry { id: "sounds".into(), scope: "app".into(), bytes, files });
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub async fn cache_usage(app: AppHandle) -> AppResult<Vec<CacheEntry>> {
    tauri::async_runtime::spawn_blocking(move || usage(&app)).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

/// Очистить кэш по имени из `cache_usage`. Другие имена не принимаются: чистится только кэш.
#[tauri::command]
#[specta::specta]
pub fn cache_clear(app: AppHandle, id: String) -> AppResult<()> {
    let path = if id == "sounds" {
        app_dir(&app, "sounds")?
    } else {
        let kind = Kind::ALL.into_iter().find(|k| k.id() == id).ok_or_else(|| AppError::new("invalidCache", id.clone()))?;
        let (project, _) = app.state::<ProjectState>().context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
        dir(&project, kind)
    };
    if path.is_dir() {
        std::fs::remove_dir_all(&path)?;
    }
    log::info!("cache cleared: {id}");
    if id == "thumbs" || id == "proxy" {
        let _ = tauri_specta::Event::emit(&crate::media::MediaChanged, &app);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_and_trims_only_old_frames() {
        let t = tempfile::tempdir().unwrap();
        let frames = dir(t.path(), Kind::Frames);
        std::fs::create_dir_all(frames.join("sub")).unwrap();
        std::fs::write(frames.join("a.png"), [0_u8; 10]).unwrap();
        std::fs::write(frames.join("sub/b.png"), [0_u8; 5]).unwrap();
        assert_eq!(measure(&frames), (15, 2));
        trim_on_open(t.path());
        assert!(frames.join("a.png").is_file(), "свежий кадр остается");
    }
}
