//! Модель файлов проекта и их версии. Незнакомые сервису поля таймлайна хранятся как есть
//! (`extra`) — ничего не теряется при записи.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::timeline::{self, Clip, Marker, Track};
use crate::error::{AppError, AppResult};

pub const PROJECT_VERSION: u32 = 1;
/// 2 — типизированные дорожки и клипы, стандартные дорожки у пустого таймлайна.
pub const TIMELINE_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sequence {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// 0 — по содержимому.
    #[serde(default)]
    pub duration_frames: u32,
    #[serde(default = "black")]
    pub background: String,
}

fn black() -> String {
    "#000000".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub series: Option<String>,
    pub sequence: Sequence,
    /// Включенные правила библиотеки (имена файлов).
    #[serde(default)]
    pub library_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TimelineFile {
    pub version: u32,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl TimelineFile {
    /// Новый таймлайн со стандартными дорожками.
    pub fn new() -> Self {
        Self { version: TIMELINE_VERSION, tracks: timeline::default_tracks(), ..Self::default() }
    }
}

/// Весь проект в памяти (для истории — снимок целиком: файлы небольшие).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub project: ProjectFile,
    pub timeline: TimelineFile,
}

fn version_of(v: &Value) -> u32 {
    v["version"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

/// Привести `project.json` к текущей версии; `true` — файл был старым и переписан.
pub fn migrate_project(raw: Value) -> AppResult<(ProjectFile, bool)> {
    let version = version_of(&raw);
    if version > PROJECT_VERSION {
        return Err(AppError::new("newerProject", format!("project.json version {version}")));
    }
    // Версия 0 (без поля) читается как 1: поля те же.
    let mut obj = raw.as_object().cloned().ok_or_else(|| AppError::new("badProject", "project.json is not an object"))?;
    obj.entry("version").or_insert(Value::from(PROJECT_VERSION));
    let mut file: ProjectFile = serde_json::from_value(Value::Object(obj)).map_err(|e| AppError::new("badProject", e.to_string()))?;
    let migrated = file.version != PROJECT_VERSION;
    file.version = PROJECT_VERSION;
    Ok((file, migrated || version == 0))
}

pub fn migrate_timeline(raw: Value) -> AppResult<(TimelineFile, bool)> {
    let version = version_of(&raw);
    if version > TIMELINE_VERSION {
        return Err(AppError::new("newerProject", format!("timeline.json version {version}")));
    }
    let mut file: TimelineFile = serde_json::from_value(raw).map_err(|e| AppError::new("badProject", e.to_string()))?;
    let migrated = file.version != TIMELINE_VERSION;
    // 1 → 2: у пустого таймлайна появляются стандартные дорожки.
    if version < 2 && file.tracks.is_empty() {
        file.tracks = timeline::default_tracks();
    }
    file.version = TIMELINE_VERSION;
    Ok((file, migrated))
}

fn color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Проверка проекта перед записью: ошибка — правка отклоняется целиком.
pub fn validate(s: &Snapshot) -> AppResult<()> {
    let p = &s.project;
    if p.name.trim().is_empty() || p.name.len() > 120 {
        return Err(AppError::new("invalidName", "project name"));
    }
    let q = &p.sequence;
    let size_ok = (16..=8192).contains(&q.width) && (16..=8192).contains(&q.height) && q.width.is_multiple_of(2) && q.height.is_multiple_of(2);
    if !size_ok || !(1..=240).contains(&q.fps) || !color(&q.background) || q.duration_frames > 240 * 60 * 60 * 6 {
        return Err(AppError::new("invalidSequence", format!("{}x{} {} fps {}", q.width, q.height, q.fps, q.background)));
    }
    timeline::validate(&s.timeline.tracks, &s.timeline.clips)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_without_version_is_migrated() {
        let (p, migrated) = migrate_project(json!({ "name": "A", "sequence": { "width": 1080, "height": 1920, "fps": 60 } })).unwrap();
        assert!(migrated);
        assert_eq!((p.version, p.sequence.background.as_str()), (PROJECT_VERSION, "#000000"));
        let (_, again) = migrate_project(serde_json::to_value(&p).unwrap()).unwrap();
        assert!(!again);
    }

    #[test]
    fn newer_files_are_refused() {
        assert_eq!(migrate_project(json!({ "version": 99, "name": "A", "sequence": { "width": 2, "height": 2, "fps": 1 } })).unwrap_err().code, "newerProject");
        assert_eq!(migrate_timeline(json!({ "version": 99 })).unwrap_err().code, "newerProject");
    }

    #[test]
    fn unknown_timeline_items_survive() {
        let (t, migrated) = migrate_timeline(json!({ "version": 1, "clips": [], "future": { "a": [1, 2] } })).unwrap();
        assert!(migrated);
        assert_eq!(t.tracks.len(), 6, "стандартные дорожки");
        assert_eq!(serde_json::to_value(&t).unwrap()["future"]["a"], json!([1, 2]));
    }

    #[test]
    fn validation() {
        let (project, _) = migrate_project(json!({ "version": 1, "name": "A", "sequence": { "width": 1080, "height": 1920, "fps": 60, "background": "#1A1730" } })).unwrap();
        let mut s = Snapshot { project, timeline: TimelineFile::default() };
        assert!(validate(&s).is_ok());
        s.project.sequence.width = 1081;
        assert_eq!(validate(&s).unwrap_err().code, "invalidSequence", "нечетная ширина не кодируется H.264");
        s.project.sequence.width = 1080;
        s.project.sequence.background = "red".into();
        assert!(validate(&s).is_err());
    }
}
