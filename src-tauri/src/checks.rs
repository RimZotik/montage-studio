//! Проверка системы для главной и строки состояния: найдены ли Claude Code, Node.js, ffmpeg,
//! ffprobe (сначала в папке компонентов приложения, потом в PATH) и их версии; железо ПК
//! (процессор, память, видеокарта) для настроек рендера по умолчанию.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::components::{self, ComponentId};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolId {
    Claude,
    Node,
    Ffmpeg,
    Ffprobe,
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    /// Папка компонентов приложения.
    Components,
    /// Найден в PATH.
    System,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: ToolId,
    pub found: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub source: Option<ToolSource>,
    /// Файл есть, но не запустился (текст ошибки).
    pub error: Option<String>,
    /// Компонент, который ставит эту программу (у Claude Code — нет).
    pub component: Option<ComponentId>,
    /// Закрепленная версия компонента.
    pub latest: Option<String>,
    /// Стоит из папки компонентов, но старее закрепленной.
    pub outdated: bool,
    /// Размер загрузки компонента, МБ.
    pub download_mb: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub cpu: String,
    pub threads: u32,
    /// Память, ГБ (округлено).
    pub memory_gb: u32,
    pub gpus: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    pub tools: Vec<ToolStatus>,
    pub hardware: Hardware,
}

/// Без окна консоли при запуске внешней программы.
#[cfg(windows)]
fn quiet(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000)
}

#[cfg(not(windows))]
fn quiet(command: &mut Command) -> &mut Command {
    command
}

struct Spec {
    id: ToolId,
    exe: &'static str,
    /// Путь внутри папки компонентов.
    bundled: &'static str,
    version_arg: &'static str,
}

const SPECS: [Spec; 4] = [
    Spec { id: ToolId::Claude, exe: "claude", bundled: "", version_arg: "--version" },
    Spec { id: ToolId::Node, exe: "node", bundled: "node/node.exe", version_arg: "--version" },
    Spec { id: ToolId::Ffmpeg, exe: "ffmpeg", bundled: "ffmpeg/bin/ffmpeg.exe", version_arg: "-version" },
    Spec { id: ToolId::Ffprobe, exe: "ffprobe", bundled: "ffmpeg/bin/ffprobe.exe", version_arg: "-version" },
];

/// Версия из первой строки вывода: `2.1.316 (Claude Code)`, `v24.18.0`,
/// `ffmpeg version 7.1-full_build-… Copyright …`.
pub fn parse_version(id: ToolId, output: &str) -> Option<String> {
    let line = output.lines().find(|l| !l.trim().is_empty())?.trim();
    let token = match id {
        ToolId::Ffmpeg | ToolId::Ffprobe => line.split_whitespace().skip_while(|w| *w != "version").nth(1)?,
        ToolId::Node => line.trim_start_matches('v'),
        ToolId::Claude => line.split_whitespace().next()?,
    };
    // `v24.18.0` у Node, `n7.1` у части сборок ffmpeg.
    let token = token.trim_start_matches(['v', 'n']);
    token.chars().next().filter(char::is_ascii_digit).map(|_| token.to_owned())
}

fn locate(spec: &Spec, components: &Path) -> Option<(PathBuf, ToolSource)> {
    if !spec.bundled.is_empty() {
        let path = components.join(spec.bundled);
        if path.is_file() {
            return Some((path, ToolSource::Components));
        }
    }
    which::which(spec.exe).ok().map(|p| (p, ToolSource::System))
}

fn component_of(id: ToolId) -> Option<ComponentId> {
    match id {
        ToolId::Claude => None,
        ToolId::Node => Some(ComponentId::Node),
        ToolId::Ffmpeg | ToolId::Ffprobe => Some(ComponentId::Ffmpeg),
    }
}

fn check(spec: &Spec, components: &Path) -> ToolStatus {
    let component = component_of(spec.id);
    let latest = component.map(|c| components::manifest(c).version.to_owned());
    let download_mb = component.map(|c| u32::try_from(components::manifest(c).size.div_ceil(1 << 20)).unwrap_or(0));
    let base = ToolStatus { id: spec.id, found: false, version: None, path: None, source: None, error: None, component, latest: latest.clone(), outdated: false, download_mb };
    let Some((path, source)) = locate(spec, components) else { return base };
    let outdated = source == ToolSource::Components && component.is_some_and(|c| components::installed_version(components, c) != latest);
    let base = ToolStatus { found: true, path: Some(path.display().to_string()), source: Some(source), outdated, ..base };
    match quiet(Command::new(&path).arg(spec.version_arg)).output() {
        Ok(out) => {
            let text = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout }).into_owned();
            let version = parse_version(spec.id, &text);
            let error = (!out.status.success() || version.is_none()).then(|| text.lines().next().unwrap_or("no output").to_owned());
            ToolStatus { version, error, ..base }
        }
        Err(e) => ToolStatus { error: Some(e.to_string()), ..base },
    }
}

fn gpus() -> Vec<String> {
    let output = quiet(Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", "(Get-CimInstance Win32_VideoController).Name"])).output();
    output
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

fn hardware() -> Hardware {
    let Ok(mut sys) = sysinfo::System::new() else {
        return Hardware { cpu: String::new(), threads: 0, memory_gb: 0, gpus: gpus() };
    };
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let cpus = sys.cpus();
    Hardware {
        cpu: cpus.first().map(|c| c.brand().trim().to_owned()).unwrap_or_default(),
        threads: u32::try_from(cpus.len()).unwrap_or(0),
        memory_gb: u32::try_from((sys.total_memory() + (1 << 29)) >> 30).unwrap_or(0),
        gpus: gpus(),
    }
}

pub fn components_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let base = app.path().app_local_data_dir().map_err(|e| AppError::new("paths", e.to_string()))?;
    Ok(base.join("components"))
}

/// Путь к найденной программе (для запуска другими частями).
pub fn find(app: &AppHandle, id: ToolId) -> Option<PathBuf> {
    let components = components_dir(app).ok()?;
    SPECS.iter().find(|s| s.id == id).and_then(|s| locate(s, &components)).map(|(p, _)| p)
}

/// Проверить все программы и железо (в фоне: запуск программ занимает до секунды).
#[tauri::command]
#[specta::specta]
pub async fn system_check(app: AppHandle) -> AppResult<SystemReport> {
    let components = components_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let tools = SPECS.iter().map(|s| check(s, &components)).collect();
        let report = SystemReport { tools, hardware: hardware() };
        for t in &report.tools {
            log::info!("check {:?}: found={} version={:?} source={:?} error={:?}", t.id, t.found, t.version, t.source, t.error);
        }
        report
    })
    .await
    .map_err(|e| AppError::new("internal", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version(ToolId::Claude, "2.1.316 (Claude Code)\n").as_deref(), Some("2.1.316"));
        assert_eq!(parse_version(ToolId::Node, "v24.18.0").as_deref(), Some("24.18.0"));
        assert_eq!(parse_version(ToolId::Ffmpeg, "ffmpeg version 7.1-full_build-www.gyan.dev Copyright (c) 2000").as_deref(), Some("7.1-full_build-www.gyan.dev"));
        assert_eq!(parse_version(ToolId::Ffprobe, "ffprobe version n7.1 Copyright").as_deref(), Some("7.1"));
        assert_eq!(parse_version(ToolId::Ffmpeg, "ffmpeg version git-2026 Copyright"), None, "не с цифры — не версия");
        assert_eq!(parse_version(ToolId::Claude, "error: not logged in"), None);
    }
}
