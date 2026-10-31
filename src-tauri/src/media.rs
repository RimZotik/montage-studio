//! Медиа проекта (`docs/project-format.md`): импорт файлов в `media/<категория>/` (копия, имя без
//! повторов), список с данными ffprobe (длительность, размер кадра, есть ли звук) и миниатюрами
//! ffmpeg; кэш — `.studio/cache/` (по пути, размеру и времени файла).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::checks::{ToolId, find};
use crate::error::{AppError, AppResult};
use crate::project::ProjectState;

pub const CATEGORIES: [&str; 5] = ["footage", "voice", "music", "sfx", "images"];
/// Звук длиннее — музыка, короче — звуки (голос пользователь или Claude переносит сам).
const MUSIC_FROM_S: f64 = 45.0;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub duration_s: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio: bool,
    pub video: bool,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    /// Путь внутри проекта: `media/footage/clip.mp4`.
    pub path: String,
    pub name: String,
    pub category: String,
    #[specta(type = specta_typescript::Number)]
    pub size: u64,
    pub probe: Option<Probe>,
    /// Миниатюра (полный путь к jpg в кэше проекта).
    pub thumb: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct ProbeCache {
    version: u32,
    /// Ключ — путь, размер и время изменения.
    items: HashMap<String, Probe>,
}

#[cfg(windows)]
fn quiet(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000)
}

#[cfg(not(windows))]
fn quiet(command: &mut Command) -> &mut Command {
    command
}

/// Категория по расширению (звук уточняется по длительности при импорте).
pub fn category_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => Some("footage"),
        "wav" | "mp3" | "flac" | "ogg" | "m4a" | "aac" | "opus" => Some("sfx"),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" => Some("images"),
        _ => None,
    }
}

/// Свободное имя в папке: `clip.mp4`, `clip-2.mp4`, …
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = name.rsplit_once('.').map_or((name, String::new()), |(s, e)| (s, format!(".{e}")));
    let mut path = dir.join(name);
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}{ext}"));
        n += 1;
    }
    path
}

pub fn probe(ffprobe: &Path, file: &Path) -> Option<Probe> {
    let out = quiet(Command::new(ffprobe).args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height", "-of", "json"]).arg(file)).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams.iter().find(|s| s["codec_type"] == "video");
    Some(Probe {
        duration_s: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()),
        width: video.and_then(|s| s["width"].as_u64()).and_then(|n| u32::try_from(n).ok()),
        height: video.and_then(|s| s["height"].as_u64()).and_then(|n| u32::try_from(n).ok()),
        audio: streams.iter().any(|s| s["codec_type"] == "audio"),
        video: video.is_some(),
    })
}

fn thumb(ffmpeg: &Path, file: &Path, out: &Path, at_s: f64) -> bool {
    if out.is_file() {
        return true;
    }
    let _ = std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")));
    quiet(Command::new(ffmpeg).args(["-v", "error", "-y", "-ss", &format!("{at_s:.2}"), "-i"]).arg(file).args(["-frames:v", "1", "-vf", "scale=320:-2"]).arg(out))
        .status()
        .is_ok_and(|s| s.success())
        && out.is_file()
}

fn key(rel: &str, meta: &std::fs::Metadata) -> String {
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    format!("{rel}|{}|{mtime}", meta.len())
}

/// Список медиа проекта (с данными ffprobe и миниатюрами, если ffmpeg есть).
pub fn list(dir: &Path, ffprobe: Option<&Path>, ffmpeg: Option<&Path>) -> AppResult<Vec<MediaItem>> {
    let cache_dir = dir.join(".studio").join("cache");
    let cache_file = cache_dir.join("probe.json");
    let mut cache: ProbeCache = std::fs::read(&cache_file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut dirty = false;
    let mut items = Vec::new();
    for cat in CATEGORIES {
        let Ok(entries) = std::fs::read_dir(dir.join("media").join(cat)) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = format!("media/{cat}/{name}");
            let k = key(&rel, &meta);
            let probe = match (cache.items.get(&k), ffprobe) {
                (Some(p), _) => Some(p.clone()),
                (None, Some(fp)) if cat != "images" => {
                    let p = probe(fp, &entry.path());
                    if let Some(p) = &p {
                        cache.items.insert(k.clone(), p.clone());
                        dirty = true;
                    }
                    p
                }
                _ => None,
            };
            let mut hasher = std::hash::DefaultHasher::new();
            std::hash::Hash::hash(&k, &mut hasher);
            let thumb_path = cache_dir.join("thumbs").join(format!("{:016x}.jpg", std::hash::Hasher::finish(&hasher)));
            let thumb = match (cat, ffmpeg) {
                ("footage" | "images", Some(ff)) => {
                    let at = probe.as_ref().and_then(|p| p.duration_s).map_or(0.0, |d| (d * 0.1).min(2.0));
                    thumb(ff, &entry.path(), &thumb_path, at).then(|| thumb_path.display().to_string())
                }
                _ => None,
            };
            items.push(MediaItem { path: rel, name, category: cat.to_owned(), size: meta.len(), probe, thumb });
        }
    }
    if dirty {
        cache.version = 1;
        std::fs::create_dir_all(&cache_dir)?;
        crate::project::write_atomic(&cache_file, &serde_json::to_vec(&cache).unwrap_or_default())?;
    }
    items.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(items)
}

/// Скопировать файлы в проект; неподходящие — пропустить. Возвращает число импортированных.
pub fn import(dir: &Path, files: &[String], ffprobe: Option<&Path>) -> AppResult<u32> {
    let mut count = 0;
    for file in files {
        let src = Path::new(file);
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(mut cat) = category_of(&name) else { continue };
        if !src.is_file() {
            continue;
        }
        if cat == "sfx" && ffprobe.and_then(|fp| probe(fp, src)).and_then(|p| p.duration_s).is_some_and(|d| d >= MUSIC_FROM_S) {
            cat = "music";
        }
        let target_dir = dir.join("media").join(cat);
        std::fs::create_dir_all(&target_dir)?;
        let target = free_name(&target_dir, &name);
        std::fs::copy(src, &target).map_err(|e| AppError::new("io", format!("{}: {e}", src.display())))?;
        count += 1;
    }
    Ok(count)
}

fn open_dir(project: &ProjectState) -> AppResult<PathBuf> {
    project.context().map(|(d, _)| d).ok_or_else(|| AppError::new("noProject", "no project"))
}

#[tauri::command]
#[specta::specta]
pub async fn media_list(app: AppHandle, project: State<'_, ProjectState>) -> AppResult<Vec<MediaItem>> {
    let dir = open_dir(&project)?;
    let (fp, ff) = (find(&app, ToolId::Ffprobe), find(&app, ToolId::Ffmpeg));
    tauri::async_runtime::spawn_blocking(move || list(&dir, fp.as_deref(), ff.as_deref())).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command]
#[specta::specta]
pub async fn media_import(app: AppHandle, project: State<'_, ProjectState>, files: Vec<String>) -> AppResult<u32> {
    let dir = open_dir(&project)?;
    let fp = find(&app, ToolId::Ffprobe);
    let n = tauri::async_runtime::spawn_blocking(move || import(&dir, &files, fp.as_deref())).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    log::info!("media imported: {n}");
    Ok(n)
}

/// Перенести файл в другую категорию (например, звук → голос).
#[tauri::command]
#[specta::specta]
pub fn media_move(project: State<'_, ProjectState>, path: String, category: String) -> AppResult<()> {
    let dir = open_dir(&project)?;
    if !CATEGORIES.contains(&category.as_str()) || !path.starts_with("media/") || path.contains("..") {
        return Err(AppError::new("invalidInput", path));
    }
    let src = dir.join(&path);
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let target_dir = dir.join("media").join(&category);
    std::fs::create_dir_all(&target_dir)?;
    std::fs::rename(&src, free_name(&target_dir, &name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_and_free_names() {
        assert_eq!(category_of("A.MP4"), Some("footage"));
        assert_eq!(category_of("take.wav"), Some("sfx"));
        assert_eq!(category_of("cover.webp"), Some("images"));
        assert_eq!(category_of("notes.txt"), None);
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.mp4"), b"x").unwrap();
        assert!(free_name(tmp.path(), "a.mp4").ends_with("a-2.mp4"));
        assert!(free_name(tmp.path(), "b.mp4").ends_with("b.mp4"));
    }

    #[test]
    fn import_and_list_without_ffmpeg() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        for f in ["clip.mp4", "hit.wav", "cover.png", "readme.txt"] {
            std::fs::write(src.join(f), b"data").unwrap();
        }
        let proj = tmp.path().join("p");
        let files: Vec<String> = ["clip.mp4", "hit.wav", "cover.png", "readme.txt", "clip.mp4"].iter().map(|f| src.join(f).display().to_string()).collect();
        assert_eq!(import(&proj, &files, None).unwrap(), 4);
        let items = list(&proj, None, None).unwrap();
        let paths: Vec<_> = items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["media/footage/clip-2.mp4", "media/footage/clip.mp4", "media/images/cover.png", "media/sfx/hit.wav"]);
    }
}
