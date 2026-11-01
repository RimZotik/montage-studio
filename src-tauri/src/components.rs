//! Установка компонентов (`docs/components.md`): закрепленная версия, загрузка с докачкой,
//! проверка SHA-256, распаковка во временную папку и подмена одним переименованием — половинчатой
//! установки не бывает. Ход — событием `ComponentProgress`. Claude Code приложение не ставит
//! (только находит, `checks.rs`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::checks::components_dir;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ComponentId {
    Node,
    Ffmpeg,
    /// Модель распознавания речи (необязательный: ставится, когда понадобилась).
    Speech,
}

/// Имя файла модели в папке компонента.
pub const SPEECH_MODEL: &str = "model.bin";

pub struct Manifest {
    pub id: ComponentId,
    pub version: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    /// Файл внутри установленной папки, по которому видно, что установка цела.
    pub check: &'static str,
    /// Один файл, а не архив: кладется как `check` без распаковки.
    pub raw: bool,
}

/// Закрепленные версии. Обновление — новая строка с суммой из официальной публикации.
pub const MANIFEST: [Manifest; 3] = [
    Manifest {
        id: ComponentId::Node,
        version: "24.22.2",
        url: "https://nodejs.org/dist/v24.22.2/node-v24.22.2-win-x64.zip",
        sha256: "ca1bff5d8ead13c9f2ea404a176db46bd430704d5b5113734049fa365e7799e2",
        size: 37_754_834,
        check: "node.exe",
        raw: false,
    },
    Manifest {
        id: ComponentId::Ffmpeg,
        version: "9.0.2",
        url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-full_build-shared.zip",
        sha256: "8d31e162f1616e37aab3fa2db991b97e1b8dbeb1c8465fd81a81be16ff91b328",
        size: 99_899_672,
        check: "bin/ffmpeg.exe",
        raw: false,
    },
    Manifest {
        id: ComponentId::Speech,
        version: "large-v3-turbo-q5_0",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size: 574_041_195,
        check: SPEECH_MODEL,
        raw: true,
    },
];

pub fn manifest(id: ComponentId) -> &'static Manifest {
    MANIFEST.iter().find(|m| m.id == id).unwrap_or(&MANIFEST[0])
}

/// Имя папки компонента.
pub fn dir_name(id: ComponentId) -> &'static str {
    match id {
        ComponentId::Node => "node",
        ComponentId::Ffmpeg => "ffmpeg",
        ComponentId::Speech => "speech",
    }
}

/// Версия, записанная при установке (`.installed` в папке компонента).
pub fn installed_version(components: &Path, id: ComponentId) -> Option<String> {
    std::fs::read_to_string(components.join(dir_name(id)).join(".installed")).ok().map(|s| s.trim().to_owned())
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Download,
    Verify,
    Extract,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct ComponentProgress {
    pub id: ComponentId,
    pub phase: Phase,
    #[specta(type = specta_typescript::Number)]
    pub received: u64,
    #[specta(type = specta_typescript::Number)]
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Installs(Mutex<HashMap<ComponentId, Arc<AtomicBool>>>);

fn emit(app: &AppHandle, id: ComponentId, phase: Phase, received: u64, total: u64, error: Option<String>) {
    let _ = ComponentProgress { id, phase, received, total, error }.emit(app);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 файла.
pub fn file_sha256(path: &Path) -> AppResult<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Распаковать архив в `dest`, убрав общий верхний каталог (`ffmpeg-9.0.2-…/bin/…` → `bin/…`).
pub fn extract_stripped(archive: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| AppError::new("badArchive", e.to_string()))?;
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    let prefix = names
        .first()
        .and_then(|n| n.split_once('/').map(|(p, _)| format!("{p}/")))
        .filter(|p| names.iter().all(|n| n.starts_with(p.as_str())))
        .unwrap_or_default();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| AppError::new("badArchive", e.to_string()))?;
        // Безопасный путь внутри архива (без `..` и абсолютных путей).
        let Some(inner) = entry.enclosed_name() else { continue };
        let inner = inner.to_string_lossy().replace('\\', "/");
        let Some(rel) = inner.strip_prefix(&prefix).filter(|r| !r.is_empty()) else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut target = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut target)?;
        target.flush()?;
    }
    Ok(())
}

/// Загрузка с докачкой в `part`; `cancel` — прервать.
async fn download(app: &AppHandle, m: &Manifest, part: &Path, cancel: &AtomicBool) -> AppResult<()> {
    let have = std::fs::metadata(part).map(|x| x.len()).unwrap_or(0);
    let client = reqwest::Client::builder().user_agent(concat!("MontageStudio/", env!("CARGO_PKG_VERSION"))).build().map_err(|e| AppError::new("network", e.to_string()))?;
    let mut request = client.get(m.url);
    if have > 0 && have < m.size {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let response = request.send().await.map_err(|e| AppError::new("network", e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::new("network", format!("HTTP {status}")));
    }
    // 206 — сервер продолжил с места обрыва; иначе — с начала.
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut received = if resumed { have } else { 0 };
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(part)?;
    let mut stream = response.bytes_stream();
    let mut last_emit = 0u64;
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new("cancelled", "cancelled"));
        }
        let chunk = chunk.map_err(|e| AppError::new("network", e.to_string()))?;
        file.write_all(&chunk)?;
        received += chunk.len() as u64;
        if received - last_emit >= 512 * 1024 {
            last_emit = received;
            emit(app, m.id, Phase::Download, received, m.size, None);
        }
    }
    file.flush()?;
    Ok(())
}

async fn install(app: &AppHandle, m: &'static Manifest, cancel: &AtomicBool) -> AppResult<()> {
    let root = components_dir(app)?;
    std::fs::create_dir_all(&root)?;
    let name = dir_name(m.id);
    let part = root.join(format!(".{name}-{}.zip.part", m.version));
    emit(app, m.id, Phase::Download, 0, m.size, None);
    download(app, m, &part, cancel).await?;

    emit(app, m.id, Phase::Verify, m.size, m.size, None);
    let part_for_hash = part.clone();
    let sum = tauri::async_runtime::spawn_blocking(move || file_sha256(&part_for_hash)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    if sum != m.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(AppError::new("checksum", format!("expected {}, got {sum}", m.sha256)));
    }

    emit(app, m.id, Phase::Extract, m.size, m.size, None);
    let (fresh, target, old) = (root.join(format!(".{name}.new")), root.join(name), root.join(format!(".{name}.old")));
    let (part2, fresh2, version, check, raw) = (part.clone(), fresh.clone(), m.version, m.check, m.raw);
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let _ = std::fs::remove_dir_all(&fresh2);
        if raw {
            std::fs::create_dir_all(&fresh2)?;
            std::fs::rename(&part2, fresh2.join(check))?;
        } else {
            extract_stripped(&part2, &fresh2)?;
        }
        if !fresh2.join(check).is_file() {
            return Err(AppError::new("badArchive", format!("{check} missing in archive")));
        }
        std::fs::write(fresh2.join(".installed"), version)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::new("internal", e.to_string()))??;
    // Подмена: старая папка в сторону, новая на место, старая — удалить.
    let _ = std::fs::remove_dir_all(&old);
    if target.exists() {
        std::fs::rename(&target, &old)?;
    }
    if let Err(e) = std::fs::rename(&fresh, &target) {
        let _ = std::fs::rename(&old, &target);
        return Err(e.into());
    }
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_file(&part);
    Ok(())
}

/// Установить компонент (в фоне); ход — событием `ComponentProgress`.
#[tauri::command]
#[specta::specta]
pub fn component_install(app: AppHandle, installs: State<'_, Installs>, id: ComponentId) -> AppResult<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut busy = installs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if busy.contains_key(&id) {
            return Ok(());
        }
        busy.insert(id, Arc::clone(&cancel));
    }
    tauri::async_runtime::spawn(async move {
        let m = manifest(id);
        log::info!("component {id:?} {}: install started", m.version);
        let result = install(&app, m, &cancel).await;
        match &result {
            Ok(()) => {
                log::info!("component {id:?} {}: installed", m.version);
                emit(&app, id, Phase::Done, m.size, m.size, None);
            }
            Err(e) if e.code == "cancelled" => emit(&app, id, Phase::Cancelled, 0, m.size, None),
            Err(e) => emit(&app, id, Phase::Error, 0, m.size, Some(e.code.clone())),
        }
        if let Some(state) = tauri::Manager::try_state::<Installs>(&app) {
            state.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id);
        }
    });
    Ok(())
}

/// Прервать загрузку (скачанная часть остается — следующая установка продолжит с места).
#[tauri::command]
#[specta::specta]
pub fn component_cancel(installs: State<'_, Installs>, id: ComponentId) {
    if let Some(flag) = installs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_sane() {
        for m in &MANIFEST {
            assert_eq!(m.sha256.len(), 64, "{:?}", m.id);
            assert!(m.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(m.url.starts_with("https://") && (m.raw || m.url.ends_with(".zip")) && m.url.contains(m.version));
        }
    }

    #[test]
    fn hash_and_extract_strip_top_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("a.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.add_directory("pkg-1.0/bin/", o).unwrap();
            w.start_file("pkg-1.0/bin/tool.exe", o).unwrap();
            w.write_all(b"exe").unwrap();
            w.start_file("pkg-1.0/README.txt", o).unwrap();
            w.write_all(b"hi").unwrap();
            w.finish().unwrap();
        }
        let dest = tmp.path().join("out");
        extract_stripped(&archive, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("bin/tool.exe")).unwrap(), b"exe");
        assert!(dest.join("README.txt").is_file());
        std::fs::write(tmp.path().join("x"), b"abc").unwrap();
        assert_eq!(file_sha256(&tmp.path().join("x")).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// Живая загрузка, проверка суммы и распаковка каждого компонента во временную папку:
    /// `cargo test live_archives -- --ignored --nocapture`.
    #[test]
    #[ignore = "качает ~130 МБ"]
    fn live_archives() {
        for m in MANIFEST.iter().filter(|m| !m.raw) {
            live_archive(m);
        }
    }

    fn live_archive(m: &Manifest) {
        let tmp = tempfile::tempdir().unwrap();
        let _ = rustls::crypto::ring::default_provider().install_default();
        let part = tmp.path().join("node.zip");
        let bytes = reqwest::blocking::get(m.url).unwrap().bytes().unwrap();
        std::fs::write(&part, &bytes).unwrap();
        assert_eq!(bytes.len() as u64, m.size);
        assert_eq!(file_sha256(&part).unwrap(), m.sha256);
        let dir = tmp.path().join("c");
        extract_stripped(&part, &dir).unwrap();
        let exe = dir.join(m.check);
        assert!(exe.is_file(), "{:?}: {} missing", m.id, m.check);
        let arg = if m.id == ComponentId::Ffmpeg { "-version" } else { "--version" };
        let out = std::process::Command::new(&exe).arg(arg).output().unwrap();
        assert!(out.status.success(), "{:?} does not start", m.id);
        println!("{:?}: {}", m.id, String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or(""));
    }
}
