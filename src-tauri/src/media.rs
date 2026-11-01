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
    /// Самый большой промежуток между опорными кадрами в начале файла, секунд: перемотка ждет
    /// декодирование от прошлого опорного.
    #[serde(default)]
    pub keyframe_s: Option<f64>,
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
    /// Облегченная копия для монитора, если файл тяжелый (полный путь).
    pub proxy: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct ProbeCache {
    version: u32,
    /// Ключ — путь, размер и время изменения.
    items: HashMap<String, Probe>,
}

#[cfg(windows)]
pub fn quiet(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000)
}

#[cfg(not(windows))]
pub fn quiet(command: &mut Command) -> &mut Command {
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

/// Звук по имени файла: так голос и музыка сразу ложатся в свои папки.
pub fn audio_hint(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    if ["voice", "голос", "озвуч", "дикт", "narrat", "vo_", "vo-"].iter().any(|w| n.contains(w)) {
        return Some("voice");
    }
    if ["music", "музык", "track", "трек", "song", "beat", "бит", "phonk", "фонк"].iter().any(|w| n.contains(w)) {
        return Some("music");
    }
    None
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
        keyframe_s: None,
    })
}

/// Промежуток между опорными кадрами по строкам `время,флаги` из ffprobe.
pub fn keyframe_gap(lines: &str) -> Option<f64> {
    let keys: Vec<f64> = lines.lines().filter_map(|l| l.split_once(',')).filter(|(_, f)| f.starts_with('K')).filter_map(|(t, _)| t.trim().parse().ok()).collect();
    let all: Vec<f64> = lines.lines().filter_map(|l| l.split_once(',')).filter_map(|(t, _)| t.trim().parse().ok()).collect();
    let span = all.iter().copied().fold(0.0_f64, f64::max);
    let first = *keys.first()?;
    let widest = keys.windows(2).map(|w| w[1] - w[0]).fold(first, f64::max);
    // Хвост без опорных кадров — тоже ожидание.
    Some(widest.max(span - keys.last().copied().unwrap_or(0.0)))
}

/// Как часто в файле опорные кадры: смотрятся первые 30 секунд, без декодирования.
fn keyframes(ffprobe: &Path, file: &Path) -> Option<f64> {
    let out = quiet(Command::new(ffprobe).args(["-v", "error", "-select_streams", "v:0", "-read_intervals", "%+30", "-show_entries", "packet=pts_time,flags", "-of", "csv=p=0"]).arg(file)).output().ok()?;
    keyframe_gap(&String::from_utf8_lossy(&out.stdout))
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

/// Волна звука картинкой (ffmpeg `showwavespic`), цвет — акцент студии.
fn waveform(ffmpeg: &Path, file: &Path, out: &Path) -> bool {
    if out.is_file() {
        return true;
    }
    let _ = std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")));
    quiet(Command::new(ffmpeg).args(["-v", "error", "-y", "-i"]).arg(file).args(["-filter_complex", "aformat=channel_layouts=mono,showwavespic=s=1200x240:colors=#F28A4B:scale=sqrt", "-frames:v", "1"]).arg(out))
        .status()
        .is_ok_and(|s| s.success())
        && out.is_file()
}

pub fn key(rel: &str, meta: &std::fs::Metadata) -> String {
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    format!("{rel}|{}|{mtime}", meta.len())
}

/// Список медиа проекта (с данными ffprobe и миниатюрами, если ffmpeg есть).
/// Список медиа. `ffprobe`/`ffmpeg` = `None` — только готовое из кэша (быстро, для интерфейса);
/// с ними — недостающие данные и миниатюры строятся (фоном, `media_list`).
pub fn list(dir: &Path, ffprobe: Option<&Path>, ffmpeg: Option<&Path>) -> AppResult<Vec<MediaItem>> {
    let cache_dir = crate::cache::root(dir);
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
            let mut probe = match (cache.items.get(&k), ffprobe) {
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
            // Опорные кадры нужны только футажу; у записи из старого кэша поля еще нет.
            if let (Some(p), Some(fp), "footage") = (probe.as_mut(), ffprobe, cat)
                && p.video
                && p.keyframe_s.is_none()
            {
                p.keyframe_s = keyframes(fp, &entry.path());
                cache.items.insert(k.clone(), p.clone());
                dirty = true;
            }
            // Имя файла кэша — SHA-256 ключа (стабильно между версиями программы).
            let digest = <sha2::Sha256 as sha2::Digest>::digest(k.as_bytes());
            let id: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
            let thumb_path = cache_dir.join("thumbs").join(format!("{id}.jpg"));
            let thumb = match (cat, ffmpeg) {
                ("footage" | "images", Some(ff)) => {
                    let at = probe.as_ref().and_then(|p| p.duration_s).map_or(0.0, |d| (d * 0.1).min(2.0));
                    thumb(ff, &entry.path(), &thumb_path, at).then(|| thumb_path.display().to_string())
                }
                ("voice" | "music" | "sfx", Some(ff)) => {
                    let wave = thumb_path.with_extension("png");
                    waveform(ff, &entry.path(), &wave).then(|| wave.display().to_string())
                }
                // Без ffmpeg — только уже построенное.
                ("footage" | "images", None) => thumb_path.is_file().then(|| thumb_path.display().to_string()),
                (_, None) => thumb_path.with_extension("png").is_file().then(|| thumb_path.with_extension("png").display().to_string()),
                _ => None,
            };
            let proxy_path = cache_dir.join("proxy").join(format!("{id}.mp4"));
            let proxy = proxy_path.is_file().then(|| proxy_path.display().to_string());
            items.push(MediaItem { path: rel, name, category: cat.to_owned(), size: meta.len(), probe, thumb, proxy });
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

/// Итог импорта: сколько добавлено и что пропущено (имя и причина: `unsupported`, `busy` —
/// файл открыт другой программой, `io`).
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: u32,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

/// Скопировать файлы в проект; файл, который не подошел или занят, не мешает остальным.
pub fn import(dir: &Path, files: &[String], ffprobe: Option<&Path>) -> AppResult<ImportResult> {
    let mut count = 0;
    let mut skipped = Vec::new();
    for file in files {
        let src = Path::new(file);
        let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(mut cat) = category_of(&name) else {
            skipped.push(Skipped { name, reason: "unsupported".into() });
            continue;
        };
        if !src.is_file() {
            continue;
        }
        if cat == "sfx" {
            if let Some(hint) = audio_hint(&name) {
                cat = hint;
            } else if ffprobe.and_then(|fp| probe(fp, src)).and_then(|p| p.duration_s).is_some_and(|d| d >= MUSIC_FROM_S) {
                cat = "music";
            }
        }
        let target_dir = dir.join("media").join(cat);
        std::fs::create_dir_all(&target_dir)?;
        let target = free_name(&target_dir, &name);
        match std::fs::copy(src, &target) {
            Ok(_) => count += 1,
            Err(e) => {
                let _ = std::fs::remove_file(&target);
                // 32 — файл открыт другой программой без права чтения.
                let reason = if e.raw_os_error() == Some(32) { "busy" } else { "io" };
                log::warn!("media import skipped {}: {e}", src.display());
                skipped.push(Skipped { name, reason: reason.into() });
            }
        }
    }
    Ok(ImportResult { imported: count, skipped })
}

fn open_dir(project: &ProjectState) -> AppResult<PathBuf> {
    project.context().map(|(d, _)| d).ok_or_else(|| AppError::new("noProject", "no project"))
}

/// Фоновая подготовка медиа идет (одна за раз).
#[derive(Default)]
pub struct MediaJobs(std::sync::atomic::AtomicBool);

/// Медиа проекта изменились (готовы миниатюры, волны, данные файлов) — перечитать список.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct MediaChanged;

/// Нужна ли облегченная копия: кадр больше Full HD, поток выше 30 Мбит/с или редкие опорные кадры —
/// перемотка такого файла ждет декодирование всех кадров от прошлого опорного. Файлу с частыми
/// опорными кадрами она не нужна: браузер и так перематывает его сразу.
pub fn wants_proxy(item: &MediaItem) -> bool {
    let Some(p) = item.probe.as_ref().filter(|p| p.video) else { return false };
    let long_side = p.width.unwrap_or(0).max(p.height.unwrap_or(0));
    let mbps = p.duration_s.filter(|d| *d > 0.0).map_or(0.0, |d| item.size as f64 * 8.0 / d / 1_000_000.0);
    item.category == "footage" && (long_side > 2160 || mbps > 30.0 || p.keyframe_s.is_some_and(|g| g > 0.75))
}

/// Копии строятся фоном, по одной: ПК не перегружается, работать можно сразу.
fn start_proxies(app: &AppHandle, dir: &Path, items: &[MediaItem]) {
    static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let todo: Vec<(PathBuf, PathBuf)> = items
        .iter()
        .filter(|i| i.proxy.is_none() && wants_proxy(i))
        .filter_map(|i| {
            let k = key(&i.path, &std::fs::metadata(dir.join(&i.path)).ok()?);
            let digest = <sha2::Sha256 as sha2::Digest>::digest(k.as_bytes());
            let id: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
            Some((dir.join(&i.path), crate::cache::dir(dir, crate::cache::Kind::Proxy).join(format!("{id}.mp4"))))
        })
        .collect();
    let Some(ffmpeg) = find(app, ToolId::Ffmpeg) else { return };
    if todo.is_empty() || BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        for (file, out) in todo {
            let _ = std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")));
            let tmp = out.with_extension("tmp.mp4");
            // Опорный кадр часто, без кадров назад, легкое декодирование: перемотка без ожидания.
            let mut c = Command::new(&ffmpeg);
            c.args(["-v", "error", "-y", "-threads", "2", "-i"]).arg(&file).args(["-vf", "scale='if(gt(iw,ih),-2,720)':'if(gt(iw,ih),720,-2)'", "-c:v", "libx264", "-preset", "veryfast", "-crf", "26", "-g", "10", "-bf", "0", "-tune", "fastdecode", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]).arg(&tmp);
            let ok = quiet(&mut c).status().is_ok_and(|s| s.success()) && std::fs::rename(&tmp, &out).is_ok();
            if ok {
                log::info!("proxy ready: {}", out.display());
                let _ = tauri_specta::Event::emit(&MediaChanged, &app2);
            } else {
                let _ = std::fs::remove_file(&tmp);
                log::warn!("proxy failed: {}", file.display());
            }
        }
        BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

#[tauri::command]
#[specta::specta]
pub async fn media_list(app: AppHandle, project: State<'_, ProjectState>, jobs: State<'_, MediaJobs>) -> AppResult<Vec<MediaItem>> {
    let dir = open_dir(&project)?;
    let dir2 = dir.clone();
    let items = tauri::async_runtime::spawn_blocking(move || list(&dir2, None, None)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    // Чего не хватает (данные файла, миниатюра, волна) — строится фоном, затем `MediaChanged`.
    start_proxies(&app, &dir, &items);
    let missing = items.iter().any(|i| (i.category != "images" && i.probe.is_none()) || i.thumb.is_none());
    let (fp, ff) = (find(&app, ToolId::Ffprobe), find(&app, ToolId::Ffmpeg));
    if missing && fp.is_some() && ff.is_some() && !jobs.0.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let app2 = app.clone();
        std::thread::spawn(move || {
            let result = list(&dir, fp.as_deref(), ff.as_deref());
            if let Some(j) = tauri::Manager::try_state::<MediaJobs>(&app2) {
                j.0.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            match result {
                Ok(_) => {
                    let _ = tauri_specta::Event::emit(&MediaChanged, &app2);
                }
                Err(e) => log::warn!("media prepare failed: {}", e.message),
            }
        });
    }
    Ok(items)
}

#[tauri::command]
#[specta::specta]
pub async fn media_import(app: AppHandle, project: State<'_, ProjectState>, files: Vec<String>) -> AppResult<ImportResult> {
    let dir = open_dir(&project)?;
    let fp = find(&app, ToolId::Ffprobe);
    let n = tauri::async_runtime::spawn_blocking(move || import(&dir, &files, fp.as_deref())).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    log::info!("media imported: {}, skipped: {}", n.imported, n.skipped.len());
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
mod keyframe_tests {
    use super::keyframe_gap;

    #[test]
    fn gap_is_the_widest_wait() {
        assert_eq!(keyframe_gap("0.0,K__\n0.5,___\n1.0,K__\n1.5,___\n"), Some(1.0));
        assert_eq!(keyframe_gap("0.0,K__\n1.0,___\n5.0,___\n"), Some(5.0), "один опорный на весь отрезок");
        assert_eq!(keyframe_gap(""), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_names_give_a_hint() {
        assert_eq!(audio_hint("Голос дубль 2.wav"), Some("voice"));
        assert_eq!(audio_hint("phonk_mix.mp3"), Some("music"));
        assert_eq!(audio_hint("boom.wav"), None);
    }

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
        let r = import(&proj, &files, None).unwrap();
        assert_eq!((r.imported, r.skipped.len(), r.skipped[0].reason.as_str()), (4, 1, "unsupported"));
        let items = list(&proj, None, None).unwrap();
        let paths: Vec<_> = items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["media/footage/clip-2.mp4", "media/footage/clip.mp4", "media/images/cover.png", "media/sfx/hit.wav"]);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// С установленным компонентом ffmpeg: данные файла, миниатюра и волна строятся.
    /// `cargo test live_media_tools -- --ignored --nocapture`.
    #[test]
    #[ignore = "нужен ffmpeg из компонентов"]
    fn live_media_tools() {
        let bin = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join("app.montagestudio.desktop/components/ffmpeg/bin");
        let (ff, fp) = (bin.join("ffmpeg.exe"), bin.join("ffprobe.exe"));
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let make = |args: &[&str], out: &str| assert!(Command::new(&ff).args(["-v", "error", "-y"]).args(args).arg(src.join(out)).status().unwrap().success());
        make(&["-f", "lavfi", "-i", "testsrc=size=1080x1920:rate=30:duration=3", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p"], "clip.mp4");
        make(&["-f", "lavfi", "-i", "sine=frequency=220:duration=60"], "bed.wav");
        let proj = tmp.path().join("p");
        let files: Vec<String> = ["clip.mp4", "bed.wav"].iter().map(|f| src.join(f).display().to_string()).collect();
        let r = import(&proj, &files, Some(&fp)).unwrap();
        assert_eq!(r.imported, 2);
        let fast = list(&proj, None, None).unwrap();
        assert!(fast.iter().all(|i| i.thumb.is_none()), "без инструментов — только кэш");
        let full = list(&proj, Some(&fp), Some(&ff)).unwrap();
        for i in &full {
            println!("{} {:?} thumb={:?}", i.path, i.probe, i.thumb.as_ref().map(|t| std::fs::metadata(t).map(|m| m.len()).unwrap_or(0)));
        }
        let clip = full.iter().find(|i| i.category == "footage").unwrap();
        assert_eq!(clip.probe.as_ref().and_then(|p| p.height), Some(1920));
        assert!(clip.thumb.is_some());
        let bed = full.iter().find(|i| i.name == "bed.wav").unwrap();
        assert_eq!(bed.category, "music", "звук от 45 с — музыка");
        assert!(bed.thumb.as_deref().is_some_and(|t| t.ends_with(".png")), "волна построена");
        assert!(list(&proj, None, None).unwrap().iter().all(|i| i.thumb.is_some()), "после фона — из кэша");
    }
}
