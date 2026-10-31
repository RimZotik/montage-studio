//! Лимиты подписки Claude (окно 5 часов и неделя). Источник — событие `rate_limit_event`, которое
//! `claude` присылает в потоке после ответа (`docs/claude-bridge.md`). Пока нет чата (с 0.4 —
//! из каждого хода), обновление — по кнопке: короткий запрос самой легкой модели. Последнее
//! известное значение хранится в SQLite (`claude_limits`) и показывается с временем.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde::Serialize;
use serde_json::Value;
use specta::Type;
use tauri::{AppHandle, State};

use crate::checks::{ToolId, find};
use crate::db::Db;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    /// Доля использования, 0–1.
    pub utilization: f64,
    /// Сброс, Unix (секунды).
    #[specta(type = specta_typescript::Number)]
    pub resets_at: i64,
    /// `allowed`, `allowed_warning`, `rejected`.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// Когда получены, Unix (секунды).
    #[specta(type = specta_typescript::Number)]
    pub at: i64,
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
}

fn window(v: &Value) -> Option<LimitWindow> {
    Some(LimitWindow { utilization: v["utilization"].as_f64()?, resets_at: v["resetsAt"].as_i64()?, status: v["status"].as_str().unwrap_or("allowed").to_owned() })
}

/// Лимиты из строки потока, если это `rate_limit_event`.
pub fn parse_event(line: &str, at: i64) -> Option<Limits> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v["type"] != "rate_limit_event" {
        return None;
    }
    let windows = &v["rate_limit_info"]["unifiedWindows"];
    Some(Limits { at, five_hour: window(&windows["five_hour"]), seven_day: window(&windows["seven_day"]) })
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

pub fn store(db: &Db, l: &Limits) -> AppResult<()> {
    let f = l.five_hour.as_ref();
    let s = l.seven_day.as_ref();
    db.with(|c| {
        c.execute(
            "insert into claude_limits (id, at, five_hour, five_hour_resets, five_hour_status, seven_day, seven_day_resets, seven_day_status)
             values (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
             on conflict(id) do update set at = excluded.at, five_hour = excluded.five_hour, five_hour_resets = excluded.five_hour_resets,
               five_hour_status = excluded.five_hour_status, seven_day = excluded.seven_day, seven_day_resets = excluded.seven_day_resets,
               seven_day_status = excluded.seven_day_status",
            rusqlite::params![l.at, f.map(|w| w.utilization), f.map(|w| w.resets_at), f.map(|w| w.status.clone()), s.map(|w| w.utilization), s.map(|w| w.resets_at), s.map(|w| w.status.clone())],
        )?;
        Ok(())
    })
}

pub fn last(db: &Db) -> AppResult<Option<Limits>> {
    db.with(|c| {
        let mut stmt = c.prepare("select at, five_hour, five_hour_resets, five_hour_status, seven_day, seven_day_resets, seven_day_status from claude_limits where id = 1")?;
        let mut rows = stmt.query([])?;
        let Some(r) = rows.next()? else { return Ok(None) };
        let w = |u: Option<f64>, t: Option<i64>, s: Option<String>| Some(LimitWindow { utilization: u?, resets_at: t?, status: s.unwrap_or_default() });
        Ok(Some(Limits { at: r.get(0)?, five_hour: w(r.get(1)?, r.get(2)?, r.get(3)?), seven_day: w(r.get(4)?, r.get(5)?, r.get(6)?) }))
    })
}

/// Короткий запрос к Claude ради события лимитов (в пустой временной папке, легкая модель).
fn probe(claude: &std::path::Path) -> AppResult<Limits> {
    let dir = std::env::temp_dir().join("montage-studio-probe");
    std::fs::create_dir_all(&dir)?;
    let mut command = Command::new(claude);
    command
        .current_dir(&dir)
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--model", "haiku"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| AppError::new("claudeStart", e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let msg = r#"{"type":"user","message":{"role":"user","content":"Reply with one word: ok"}}"#;
        writeln!(stdin, "{msg}").map_err(|e| AppError::new("claudeStart", e.to_string()))?;
    }
    let stdout = child.stdout.take().ok_or_else(|| AppError::new("claudeStart", "no stdout"))?;
    let mut found = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        if found.is_none() {
            found = parse_event(&line, now());
        }
        if line.contains(r#""type":"result""#) {
            break;
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    found.ok_or_else(|| AppError::new("claudeNoLimits", "claude did not report limits (not logged in with a subscription?)"))
}

#[tauri::command]
#[specta::specta]
pub fn claude_limits_last(db: State<'_, Db>) -> AppResult<Option<Limits>> {
    last(&db)
}

/// Обновить лимиты коротким запросом (по кнопке).
#[tauri::command]
#[specta::specta]
pub async fn claude_limits_refresh(app: AppHandle, db: State<'_, Db>) -> AppResult<Limits> {
    let claude = find(&app, ToolId::Claude).ok_or_else(|| AppError::new("claudeMissing", "claude not found"))?;
    let limits = tauri::async_runtime::spawn_blocking(move || probe(&claude)).await.map_err(|e| AppError::new("internal", e.to_string()))??;
    store(&db, &limits)?;
    Ok(limits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: &str = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":1793887200,"rateLimitType":"seven_day","utilization":0.78,"unifiedWindows":{"five_hour":{"utilization":0.33,"resetsAt":1793482800,"status":"allowed"},"seven_day":{"utilization":0.78,"resetsAt":1793887200,"status":"allowed_warning","surpassedThreshold":0.75}}},"session_id":"x"}"#;

    #[test]
    fn parses_and_stores() {
        let l = parse_event(EVENT, 10).unwrap();
        assert_eq!(l.five_hour.as_ref().unwrap().utilization, 0.33);
        assert_eq!(l.seven_day.as_ref().unwrap().status, "allowed_warning");
        assert!(parse_event(r#"{"type":"result"}"#, 1).is_none());
        let db = Db::memory().unwrap();
        assert_eq!(last(&db).unwrap(), None);
        store(&db, &l).unwrap();
        assert_eq!(last(&db).unwrap(), Some(l));
    }

    #[test]
    fn missing_window_is_none() {
        let l = parse_event(r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.1,"resetsAt":5}}}}"#, 1).unwrap();
        assert!(l.seven_day.is_none());
        assert_eq!(l.five_hour.unwrap().status, "allowed");
    }
}
