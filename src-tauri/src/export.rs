//! Экспорт (`docs/export.md`): та же композиция, что играет в мониторе, рендерится движком
//! (Remotion в Node.js, компонент `components/render`). Настройки — пресеты качества для соцсетей,
//! а не сотни параметров: H.264 в yuv420p, BT.709, AAC и быстрый старт подходят всем площадкам.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::checks::{ToolId, components_dir, find};
use crate::error::{AppError, AppResult};
use crate::media::quiet;
use crate::project::ProjectState;

/// Версия движка: совпадает с версией `remotion` в интерфейсе и в `resources/render/package.json`.
const ENGINE_VERSION: &str = "4.0.539";
const PACKAGE_JSON: &str = include_str!("../resources/render/package.json");
const PACKAGE_LOCK: &str = include_str!("../resources/render/package-lock.json");
const RUNNER: &str = include_str!("../resources/render/run.mjs");

/// Настройки экспорта.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportSettings {
    /// Имя файла без расширения; пусто — по проекту.
    pub name: String,
    /// `h264` (подходит всем) или `h265` (меньше файл, но не все площадки и плееры).
    pub codec: String,
    /// `max`, `high` (по умолчанию) или `compact`.
    pub quality: String,
    /// Размер кадра относительно последовательности: 1, 0.5.
    pub scale: f64,
}

/// `.studio/exports.json`: куда складывать ролики проекта и что уже готово (файлы могут лежать
/// где угодно, поэтому список не выводится из папки).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Journal {
    version: u32,
    dir: Option<String>,
    files: Vec<String>,
}

fn journal_path(dir: &Path) -> PathBuf {
    dir.join(".studio").join("exports.json")
}

fn journal(dir: &Path) -> Journal {
    std::fs::read(journal_path(dir)).ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default()
}

fn journal_write(dir: &Path, j: &Journal) -> AppResult<()> {
    if let Some(p) = journal_path(dir).parent() {
        std::fs::create_dir_all(p)?;
    }
    let body = serde_json::to_string_pretty(&Journal { version: 1, ..j.clone() }).unwrap_or_default();
    std::fs::write(journal_path(dir), body)?;
    Ok(())
}

/// Папка готовых роликов: выбранная пользователем или `output/` проекта.
pub fn out_dir(dir: &Path) -> PathBuf {
    journal(dir).dir.map_or_else(|| dir.join("output"), PathBuf::from)
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { name: String::new(), codec: "h264".into(), quality: "high".into(), scale: 1.0 }
    }
}

/// Параметры кодирования.
#[derive(Debug, Clone, PartialEq)]
pub struct Encoding {
    pub codec: &'static str,
    pub crf: u32,
    pub audio: &'static str,
    pub preset: &'static str,
}

/// Качество → кодирование. Площадки все равно перекодируют загруженное: `high` дает запас, чтобы
/// их сжатие не испортило картинку, при разумном размере файла.
pub fn encoding(s: &ExportSettings) -> Encoding {
    let hevc = s.codec == "h265";
    let (crf, audio, preset) = match s.quality.as_str() {
        "max" => (15, "320k", "medium"),
        "compact" => (23, "160k", "slow"),
        _ => (18, "256k", "medium"),
    };
    Encoding { codec: if hevc { "h265" } else { "h264" }, crf: if hevc { crf + 5 } else { crf }, audio, preset }
}

/// Ход экспорта и установки движка.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    /// `install`, `render`, `encode`, `finish`, `done`, `error`, `cancelled`.
    pub phase: String,
    /// 0–1.
    pub progress: f64,
    pub message: Option<String>,
    pub output: Option<String>,
}

fn emit(app: &AppHandle, phase: &str, progress: f64, message: Option<String>, output: Option<String>) {
    let _ = ExportProgress { phase: phase.into(), progress, message, output }.emit(app);
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    /// Файлы субтитров рядом с роликом: расширения.
    pub captions: Vec<String>,
    pub name: String,
    pub path: String,
    /// Размер, МБ.
    pub size_mb: f64,
    pub duration_s: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Средний поток, Мбит/с.
    pub mbps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportStatus {
    /// Куда лягут новые ролики.
    pub dir: Option<String>,
    pub engine_ready: bool,
    pub node_found: bool,
    pub running: bool,
    pub files: Vec<ExportFile>,
}

#[derive(Default)]
pub struct ExportState {
    child: Arc<Mutex<Option<Child>>>,
    busy: Arc<std::sync::atomic::AtomicBool>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

fn engine_dir(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(components_dir(app)?.join("render"))
}

fn engine_ready(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(".installed")).is_ok_and(|v| v.trim() == ENGINE_VERSION) && dir.join("node_modules/@remotion/renderer/package.json").is_file()
}

/// Готовая сборка композиции: при запуске из исходников — в папке проекта разработки.
fn bundle_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/render-bundle");
    if cfg!(debug_assertions) && source.join("index.html").is_file() {
        return Ok(source);
    }
    let dir = app.path().resource_dir().map_err(|e| AppError::new("paths", e.to_string()))?.join("resources/render-bundle");
    if !dir.join("index.html").is_file() {
        return Err(AppError::new("renderBundle", dir.display().to_string()));
    }
    Ok(dir)
}

/// Запустить `run.mjs` с заданием; строки его вывода — JSON.
fn spawn_runner(app: &AppHandle, job: &Value) -> AppResult<Child> {
    let node = find(app, ToolId::Node).ok_or_else(|| AppError::new("nodeMissing", "node not found"))?;
    let dir = engine_dir(app)?;
    std::fs::write(dir.join("run.mjs"), RUNNER)?;
    let job_file = dir.join(format!("job-{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())));
    std::fs::write(&job_file, job.to_string())?;
    let mut c = Command::new(node);
    c.current_dir(&dir).arg("run.mjs").arg(&job_file).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    quiet(&mut c).spawn().map_err(|e| AppError::new("renderStart", e.to_string()))
}

/// Читать вывод до конца. `on_line` получает разобранные строки; итог — последняя ошибка.
fn pump(child: &mut Child, mut on_line: impl FnMut(&Value)) -> Result<(), String> {
    let stderr = child.stderr.take();
    let tail = Arc::new(Mutex::new(String::new()));
    let tail2 = Arc::clone(&tail);
    let log_thread = std::thread::spawn(move || {
        for line in stderr.into_iter().flat_map(|e| BufReader::new(e).lines().map_while(Result::ok)) {
            log::info!("render: {line}");
            *tail2.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = line;
        }
    });
    let mut error = None;
    for line in child.stdout.take().into_iter().flat_map(|o| BufReader::new(o).lines().map_while(Result::ok)) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["type"] == "error" {
            error = v["message"].as_str().map(str::to_owned);
        }
        on_line(&v);
    }
    let _ = log_thread.join();
    if let Some(e) = error {
        return Err(e);
    }
    let last = tail.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if child.wait().map_err(|e| e.to_string())?.success() { Ok(()) } else { Err(last) }
}

fn install_engine(app: &AppHandle) -> Result<(), String> {
    let node = find(app, ToolId::Node).ok_or("Node.js not found")?;
    let dir = engine_dir(app).map_err(|e| e.message)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("package.json"), PACKAGE_JSON).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("package-lock.json"), PACKAGE_LOCK).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join(".installed"));
    let npm = node.parent().ok_or("node path")?.join("node_modules/npm/bin/npm-cli.js");
    if !npm.is_file() {
        return Err(format!("npm not found near {}", node.display()));
    }
    emit(app, "install", 0.05, None, None);
    // `npm ci` ставит ровно то, что в файле блокировки, с проверкой сумм пакетов.
    let mut c = Command::new(&node);
    c.current_dir(&dir).arg(&npm).args(["ci", "--omit=dev", "--no-audit", "--no-fund"]);
    let out = quiet(&mut c).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("npm ci failed").to_owned());
    }
    emit(app, "install", 0.4, None, None);
    // Браузер для кадров качается средствами движка.
    let mut child = spawn_runner(app, &json!({ "mode": "prepare" })).map_err(|e| e.message)?;
    pump(&mut child, |v| {
        if v["type"] == "browser" {
            emit(app, "install", 0.4 + 0.6 * v["progress"].as_f64().unwrap_or(0.0), None, None);
        }
    })?;
    std::fs::write(dir.join(".installed"), ENGINE_VERSION).map_err(|e| e.to_string())?;
    Ok(())
}

fn free_output(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.mp4"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name}-{n}.mp4"));
        n += 1;
    }
    path
}

/// Имя файла из названия: без знаков, запрещенных в Windows.
pub fn safe_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let clean = clean.trim().trim_end_matches('.').to_owned();
    if clean.is_empty() { "video".to_owned() } else { clean }
}

/// Задание рендера из состояния проекта.
fn job(app: &AppHandle, dir: &Path, doc: &crate::project::model::Snapshot) -> AppResult<Value> {
    Ok(json!({
        "bundle": bundle_dir(app)?,
        "projectDir": dir,
        "props": { "sequence": doc.project.sequence, "tracks": doc.timeline.tracks, "clips": doc.timeline.clips, "smart": crate::smart::runnable(app, dir) },
    }))
}

/// Перенести служебные данные в начало файла: ролик начинает играть и загружаться сразу.
fn fast_start(app: &AppHandle, file: &Path) -> bool {
    let Some(ffmpeg) = find(app, ToolId::Ffmpeg) else { return false };
    let tmp = file.with_extension("faststart.mp4");
    let mut c = Command::new(ffmpeg);
    c.args(["-v", "error", "-y", "-i"]).arg(file).args(["-c", "copy", "-movflags", "+faststart"]).arg(&tmp);
    let ok = quiet(&mut c).status().is_ok_and(|s| s.success()) && std::fs::rename(&tmp, file).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

/// Запустить экспорт фоном.
pub fn start(app: &AppHandle, settings: &ExportSettings) -> AppResult<PathBuf> {
    let state = app.state::<ExportState>();
    if state.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(AppError::new("exportBusy", "export is running"));
    }
    let prepared = (|| {
        if !engine_ready(&engine_dir(app)?) {
            return Err(AppError::new("renderEngineMissing", "render engine is not installed"));
        }
        let (dir, doc) = app.state::<ProjectState>().context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
        if doc.timeline.clips.is_empty() {
            return Err(AppError::new("emptyTimeline", "nothing to export"));
        }
        let name = if settings.name.trim().is_empty() { safe_name(&doc.project.name) } else { safe_name(&settings.name) };
        let out_dir = out_dir(&dir);
        std::fs::create_dir_all(&out_dir)?;
        let output = free_output(&out_dir, &name);
        let e = encoding(settings);
        let mut j = job(app, &dir, &doc)?;
        j["mode"] = json!("video");
        j["output"] = json!(output);
        j["codec"] = json!(e.codec);
        j["crf"] = json!(e.crf);
        j["audioBitrate"] = json!(e.audio);
        j["preset"] = json!(e.preset);
        j["scale"] = json!(if settings.scale > 0.0 { settings.scale } else { 1.0 });
        let child = spawn_runner(app, &j)?;
        Ok((child, output))
    })();
    let (mut child, output) = match prepared {
        Ok(x) => x,
        Err(e) => {
            state.busy.store(false, std::sync::atomic::Ordering::SeqCst);
            return Err(e);
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    *state.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(child);
    state.cancelled.store(false, std::sync::atomic::Ordering::SeqCst);
    let (app2, out2) = (app.clone(), output.clone());
    let project_dir = app.state::<ProjectState>().context().map(|c| c.0);
    let (child_slot, busy, cancelled) = (Arc::clone(&state.child), Arc::clone(&state.busy), Arc::clone(&state.cancelled));
    std::thread::spawn(move || {
        log::info!("export started: {}", out2.display());
        emit(&app2, "render", 0.0, None, None);
        let log_thread = std::thread::spawn(move || {
            for line in stderr.into_iter().flat_map(|e| BufReader::new(e).lines().map_while(Result::ok)) {
                log::info!("render: {line}");
            }
        });
        let mut error = None;
        for line in stdout.into_iter().flat_map(|o| BufReader::new(o).lines().map_while(Result::ok)) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            match v["type"].as_str() {
                Some("progress") => emit(&app2, v["stage"].as_str().unwrap_or("render"), v["progress"].as_f64().unwrap_or(0.0), None, None),
                Some("error") => error = v["message"].as_str().map(str::to_owned),
                _ => {}
            }
        }
        let _ = log_thread.join();
        let success = child_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().and_then(|mut c| c.wait().ok()).is_some_and(|s| s.success());
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::remove_file(&out2);
            log::info!("export cancelled");
            emit(&app2, "cancelled", 0.0, None, None);
        } else if success && error.is_none() && out2.is_file() {
            emit(&app2, "finish", 1.0, None, None);
            fast_start(&app2, &out2);
            if let Some(d) = &project_dir {
                let mut j = journal(d);
                j.files.retain(|f| Path::new(f) != out2);
                j.files.push(out2.display().to_string());
                let _ = journal_write(d, &j);
            }
            if let Some(doc) = app2.state::<ProjectState>().context().map(|c| c.1)
                && crate::captions::soft_track(&doc).is_some()
                    && let Err(e) = crate::captions::write_beside(&doc, &out2) {
                        log::warn!("caption files: {}", e.message);
                    }
            log::info!("export done: {}", out2.display());
            emit(&app2, "done", 1.0, None, Some(out2.display().to_string()));
        } else {
            let message = error.unwrap_or_else(|| "render failed".into());
            let _ = AppError::new("exportFailed", message.clone());
            emit(&app2, "error", 0.0, Some(message), None);
        }
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
    });
    Ok(output)
}

/// Кадр таймлайна картинкой: Claude смотрит результат, не трогая окно пользователя.
pub fn still(app: &AppHandle, frame: u32, scale: f64) -> AppResult<PathBuf> {
    // По одному: Claude просит кадры пачкой, а несколько браузеров разом не успевают запуститься.
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !engine_ready(&engine_dir(app)?) {
        return Err(AppError::new("renderEngineMissing", "render engine is not installed"));
    }
    let (dir, doc) = app.state::<ProjectState>().context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
    let frames = crate::cache::dir(&dir, crate::cache::Kind::Frames);
    std::fs::create_dir_all(&frames)?;
    let out = frames.join(format!("f{frame}.jpg"));
    let mut j = job(app, &dir, &doc)?;
    j["mode"] = json!("still");
    j["frame"] = json!(frame);
    j["output"] = json!(out);
    j["scale"] = json!(scale);
    let mut child = spawn_runner(app, &j)?;
    pump(&mut child, |_| {}).map_err(|e| AppError::new("exportFailed", e))?;
    Ok(out)
}

/// Ролики проекта, новые первыми: из журнала и из папки (файл, сделанный раньше, тоже виден). Журнал
/// лежит в папке проекта, которую могли прислать: принимаются только существующие `.mp4`.
fn candidates(dir: &Path) -> Vec<PathBuf> {
    let is_video = |p: &Path| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mp4"));
    let mut paths: Vec<PathBuf> = journal(dir).files.iter().map(PathBuf::from).filter(|p| is_video(p)).collect();
    for folder in [dir.join("output"), out_dir(dir)] {
        for e in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            if is_video(&e.path()) && !paths.contains(&e.path()) {
                paths.push(e.path());
            }
        }
    }
    let mut dated: Vec<(std::time::SystemTime, PathBuf)> = paths.into_iter().filter(|p| p.is_file()).filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).collect();
    dated.sort_by_key(|x| std::cmp::Reverse(x.0));
    dated.into_iter().take(50).map(|x| x.1).collect()
}

fn files(app: &AppHandle, dir: &Path) -> Vec<ExportFile> {
    let ffprobe = find(app, ToolId::Ffprobe);
    let mut list: Vec<(std::time::SystemTime, ExportFile)> = candidates(dir)
        .into_iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            let _ = app.asset_protocol_scope().allow_file(&path);
            let probe = ffprobe.as_ref().and_then(|p| crate::media::probe(p, &path));
            let duration_s = probe.as_ref().and_then(|p| p.duration_s);
            let size = meta.len() as f64;
            Some((meta.modified().ok()?, ExportFile { captions: crate::captions::EXTENSIONS.iter().filter(|x| path.with_extension(x).is_file()).map(|x| (*x).to_owned()).collect(), name: path.file_name()?.to_string_lossy().into_owned(), path: path.display().to_string(), size_mb: size / 1_048_576.0, duration_s, width: probe.as_ref().and_then(|p| p.width), height: probe.as_ref().and_then(|p| p.height), mbps: duration_s.filter(|d| *d > 0.0).map(|d| size * 8.0 / d / 1_000_000.0) }))
        })
        .collect();
    list.sort_by_key(|x| std::cmp::Reverse(x.0));
    list.into_iter().map(|x| x.1).take(50).collect()
}

/// Состояние движка и готовые файлы проекта.
pub fn status(app: &AppHandle) -> AppResult<ExportStatus> {
    let dir = app.state::<ProjectState>().context().map(|c| c.0);
    Ok(ExportStatus { engine_ready: engine_ready(&engine_dir(app)?), node_found: find(app, ToolId::Node).is_some(), running: app.state::<ExportState>().busy.load(std::sync::atomic::Ordering::SeqCst), dir: dir.as_ref().map(|d| out_dir(d).display().to_string()), files: dir.map_or_else(Vec::new, |d| files(app, &d)) })
}

#[tauri::command]
#[specta::specta]
pub async fn export_status(app: AppHandle) -> AppResult<ExportStatus> {
    tauri::async_runtime::spawn_blocking(move || status(&app)).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

/// Выбрать папку готовых роликов (`None` — `output/` проекта).
#[tauri::command]
#[specta::specta]
pub fn export_set_dir(project: State<'_, ProjectState>, dir: Option<String>) -> AppResult<()> {
    let (root, _) = project.context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
    let mut j = journal(&root);
    j.dir = dir;
    journal_write(&root, &j)
}

/// Удалить готовый ролик и его файлы субтитров. Только то, что есть в списке этого проекта.
#[tauri::command]
#[specta::specta]
pub fn export_delete(app: AppHandle, path: String) -> AppResult<()> {
    let (root, _) = app.state::<ProjectState>().context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
    let file = PathBuf::from(&path);
    if !candidates(&root).contains(&file) {
        return Err(AppError::new("notExport", path));
    }
    std::fs::remove_file(&file)?;
    for x in crate::captions::EXTENSIONS {
        let _ = std::fs::remove_file(file.with_extension(x));
    }
    let mut j = journal(&root);
    j.files.retain(|f| f != &path);
    journal_write(&root, &j)
}

#[tauri::command]
#[specta::specta]
pub fn export_engine_install(app: AppHandle) -> AppResult<()> {
    std::thread::spawn(move || match install_engine(&app) {
        Ok(()) => {
            log::info!("render engine installed");
            emit(&app, "installed", 1.0, None, None);
        }
        Err(e) => {
            let _ = AppError::new("renderEngineInstall", e.clone());
            emit(&app, "error", 0.0, Some(e), None);
        }
    });
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn export_start(app: AppHandle, settings: ExportSettings) -> AppResult<String> {
    start(&app, &settings).map(|p| p.display().to_string())
}

#[tauri::command]
#[specta::specta]
pub fn export_cancel(state: State<'_, ExportState>) {
    state.cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(c) = state.child.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_mut() {
        // Вместе с браузером и кодировщиком движка: иначе они держат файл.
        #[cfg(windows)]
        {
            let mut t = Command::new("taskkill");
            t.args(["/T", "/F", "/PID", &c.id().to_string()]);
            let _ = quiet(&mut t).status();
        }
        let _ = c.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_presets() {
        let s = |codec: &str, quality: &str| ExportSettings { codec: codec.into(), quality: quality.into(), ..ExportSettings::default() };
        assert_eq!(encoding(&ExportSettings::default()), Encoding { codec: "h264", crf: 18, audio: "256k", preset: "medium" });
        assert!(encoding(&s("h264", "max")).crf < encoding(&s("h264", "compact")).crf);
        assert_eq!(encoding(&s("h265", "high")).codec, "h265");
        assert!(encoding(&s("h265", "high")).crf > 18, "у H.265 та же картинка при большем числе");
        assert_eq!(encoding(&s("что-то", "неизвестно")).codec, "h264", "неизвестное — безопасный вариант");
    }

    #[test]
    fn output_names_are_safe_and_free() {
        assert_eq!(safe_name("Второй шортс/тикток: v2?"), "Второй шортс_тикток_ v2_");
        assert_eq!(safe_name("  . "), "video");
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.mp4"), b"").unwrap();
        assert_eq!(free_output(tmp.path(), "a").file_name().unwrap(), "a-2.mp4");
        assert_eq!(free_output(tmp.path(), "b").file_name().unwrap(), "b.mp4");
    }

    #[test]
    fn engine_version_matches_the_interface() {
        let pkg: Value = serde_json::from_str(PACKAGE_JSON).unwrap();
        assert_eq!(pkg["dependencies"]["@remotion/renderer"], ENGINE_VERSION);
        let app: Value = serde_json::from_str(include_str!("../../package.json")).unwrap();
        assert_eq!(app["dependencies"]["remotion"], ENGINE_VERSION, "движок и плеер — одной версии");
    }
}
