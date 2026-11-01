//! Чат с Claude в открытом проекте (`docs/claude-bridge.md`): процесс `claude` в режиме потока
//! событий в папке проекта, без окна. Сообщение = блок контекста хода + текст; события
//! раскладываются (`events.rs`) и уходят в интерфейс (`ChatEvent`); лимиты — в индикатор; расход
//! хода — в SQLite (`claude_turns`). Окружение изолировано от личных настроек Claude Code
//! пользователя (свои правила, без чужих MCP и навыков) — меньше токенов на каждый ход.

mod events;
pub mod prompt;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::checks::{ToolId, find};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::project::ProjectState;
pub use events::{ChatEvent, Parsed};

/// Встроенные инструменты Claude в студии (до инструментов MCP студии).
const TOOLS: &str = "Read,Glob,Grep,Edit,Write";
/// Без вопроса разрешено: чтение и правка правил проекта. Остальное — отказ (видно в чате).
/// `mcp__studio` — все инструменты MCP студии (правки идут через сервис проекта с отменой).
const ALLOWED: [&str; 6] = ["Read", "Glob", "Grep", "Edit(rules/**)", "Write(rules/**)", "mcp__studio"];

/// Строка истории чата для показа (`.studio/claude/chat.jsonl`).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatItem {
    /// `user`, `assistant`, `tool`, `turn`, `decision`, `error`.
    pub role: String,
    pub text: String,
    #[specta(type = specta_typescript::Number)]
    pub at: i64,
}

struct Session {
    dir: PathBuf,
    child: Child,
    stdin: ChildStdin,
    /// Хэш постоянного контекста, с которым запущен процесс (сменился — перезапуск).
    prompt_hash: u64,
}

#[derive(Default)]
pub struct ChatState {
    session: Mutex<Option<Session>>,
    busy: Arc<std::sync::atomic::AtomicBool>,
    /// Какой список библиотеки Claude уже видел в этом процессе (хэш).
    library_seen: std::sync::atomic::AtomicU64,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

fn claude_dir(dir: &Path) -> PathBuf {
    dir.join(".studio").join("claude")
}

/// Добавить строку в историю чата проекта.
pub fn log_item(dir: &Path, role: &str, text: &str) {
    let item = ChatItem { role: role.to_owned(), text: text.to_owned(), at: now() };
    let path = claude_dir(dir).join("chat.jsonl");
    let _ = std::fs::create_dir_all(claude_dir(dir));
    if let (Ok(mut f), Ok(line)) = (std::fs::OpenOptions::new().create(true).append(true).open(path), serde_json::to_string(&item)) {
        let _ = writeln!(f, "{line}");
    }
}

fn saved_session(dir: &Path) -> Option<String> {
    let raw = std::fs::read(claude_dir(dir).join("session.json")).ok()?;
    serde_json::from_slice::<serde_json::Value>(&raw).ok()?["sessionId"].as_str().map(str::to_owned)
}

fn save_session(dir: &Path, id: &str, model: &str) {
    let _ = std::fs::write(claude_dir(dir).join("session.json"), serde_json::json!({ "version": 1, "sessionId": id, "model": model }).to_string());
}

/// Остановить `claude` вместе с дочерними процессами (на Windows `kill` останавливает только
/// сам процесс, его дочерние остались бы висеть).
fn kill(session: &mut Session) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &session.child.id().to_string()]).creation_flags(0x0800_0000).status();
    }
    let _ = session.child.kill();
    let _ = session.child.wait();
}

fn spawn(app: &AppHandle, dir: &Path, system: &Path, prompt_hash: u64, busy: Arc<std::sync::atomic::AtomicBool>) -> AppResult<Session> {
    let claude = find(app, ToolId::Claude).ok_or_else(|| AppError::new("claudeMissing", "claude not found"))?;
    let cdir = claude_dir(dir);
    let mcp = cdir.join("mcp.json");
    // Только MCP-сервер студии (адрес и ключ — новые на каждый запуск приложения).
    let servers = app.try_state::<crate::mcp::McpState>().and_then(|s| s.info()).map_or_else(
        || serde_json::json!({}),
        |i| serde_json::json!({ "studio": { "type": "http", "url": i.url, "headers": { "Authorization": format!("Bearer {}", i.token) } } }),
    );
    std::fs::write(&mcp, serde_json::json!({ "mcpServers": servers }).to_string())?;
    let mut command = build_command(&claude, dir, system, &mcp, saved_session(dir));
    let model = app.try_state::<crate::settings::SettingsState>().map(|s| s.get().claude_model).filter(|m| ["sonnet", "opus", "haiku"].contains(&m.as_str())).unwrap_or_else(|| "sonnet".into());
    command
        .args(["--model", &model])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| AppError::new("claudeStart", e.to_string()))?;
    let stdin = child.stdin.take().ok_or_else(|| AppError::new("claudeStart", "no stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| AppError::new("claudeStart", "no stdout"))?;
    let stderr = child.stderr.take();
    let (app2, dir2) = (app.clone(), dir.to_path_buf());
    std::thread::spawn(move || read_loop(&app2, &dir2, BufReader::new(stdout), &busy));
    if let Some(err) = stderr {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                log::info!("claude stderr: {line}");
            }
        });
    }
    log::info!("claude chat started in {}", dir.display());
    Ok(Session { dir: dir.to_path_buf(), child, stdin, prompt_hash })
}

/// Команда запуска `claude` для чата проекта.
pub(crate) fn build_command(claude: &Path, dir: &Path, system: &Path, mcp: &Path, resume: Option<String>) -> Command {
    let mut command = Command::new(claude);
    command
        .current_dir(dir)
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
        // Изоляция от личного окружения пользователя: только настройки проекта, без чужих MCP,
        // навыков и браузера; свой набор инструментов.
        .args(["--setting-sources", "project", "--strict-mcp-config", "--disable-slash-commands", "--no-chrome"])
        .arg("--mcp-config")
        .arg(mcp)
        .args(["--tools", TOOLS, "--allowedTools"])
        .args(ALLOWED)
        .arg("--append-system-prompt-file")
        .arg(system);
    if let Some(id) = resume {
        command.args(["--resume", &id]);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

/// Чтение потока событий процесса до его конца.
fn read_loop(app: &AppHandle, dir: &Path, out: impl BufRead, busy: &std::sync::atomic::AtomicBool) {
    let mut parser = events::Parser::default();
    for line in out.lines().map_while(Result::ok) {
        for parsed in parser.feed(&line, now()) {
            match parsed {
                Parsed::Event(event) => {
                    match &event {
                        ChatEvent::Assistant { text } if !text.trim().is_empty() => log_item(dir, "assistant", text),
                        ChatEvent::Tool { name, summary, .. } => log_item(dir, "tool", &format!("{name} {summary}")),
                        ChatEvent::Turn { .. } => {
                            busy.store(false, std::sync::atomic::Ordering::SeqCst);
                            if let Ok(text) = serde_json::to_string(&event) {
                                log_item(dir, "turn", &text);
                            }
                        }
                        _ => {}
                    }
                    let _ = event.emit(app);
                }
                Parsed::Session { id, model } => save_session(dir, &id, &model),
                Parsed::Limits(limits) => {
                    if let Some(db) = app.try_state::<Db>() {
                        let _ = crate::claude_limits::store(&db, &limits);
                    }
                    let _ = crate::claude_limits::LimitsChanged(limits).emit(app);
                }
                Parsed::Usage(turn) => {
                    if let Some(db) = app.try_state::<Db>() {
                        let _ = record_turn(&db, dir, &turn);
                    }
                }
            }
        }
    }
    if busy.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let _ = ChatEvent::Turn { ok: false, interrupted: true, duration_ms: 0, input_tokens: 0, output_tokens: 0, cost_usd: 0.0, denials: vec![] }.emit(app);
    }
    log::info!("claude chat process ended");
}

fn record_turn(db: &Db, dir: &Path, t: &events::TurnUsage) -> AppResult<()> {
    let i = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
    db.with(|c| {
        c.execute(
            "insert into claude_turns (project, session, at, model, input_tokens, output_tokens, cache_read, cache_create, cost_usd, duration_ms) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![dir.display().to_string(), t.session, now(), t.model, i(t.input), i(t.output), i(t.cache_read), i(t.cache_create), t.cost_usd, i(t.duration_ms)],
        )?;
        Ok(())
    })
}

/// Остановить процесс чата (закрытие проекта, новая сессия).
pub fn shutdown(state: &ChatState) {
    if let Some(mut s) = state.session.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
        kill(&mut s);
    }
}

/// Отправить сообщение. `kind` — `user` или `decision` (ответ на карточку: в чате показывается
/// отметкой на карточке, а не сообщением).
#[tauri::command]
#[specta::specta]
pub fn chat_send(app: AppHandle, text: String, kind: String) -> AppResult<()> {
    send(&app, &text, &kind)
}

/// Отправить сообщение в чат открытого проекта (из интерфейса или как пользователь — из
/// инструментов разработчика).
pub fn send(app: &AppHandle, text: &str, kind: &str) -> AppResult<()> {
    let chat = app.state::<ChatState>();
    let project = app.state::<ProjectState>();
    let (dir, doc) = project.context().ok_or_else(|| AppError::new("noProject", "no project"))?;
    let cdir = claude_dir(&dir);
    std::fs::create_dir_all(&cdir)?;
    let (system, hash) = prompt::system_prompt(&dir, &doc, env!("CARGO_PKG_VERSION"));
    // Сменилась модель в настройках — тоже перезапуск (с той же сессией).
    let model = app.try_state::<crate::settings::SettingsState>().map(|s| s.get().claude_model).unwrap_or_default();
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (hash, &model).hash(&mut h);
        h.finish()
    };
    let system_path = cdir.join("system.md");
    std::fs::write(&system_path, &system)?;

    let mut guard = chat.session.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let stale = guard.as_mut().is_none_or(|s| s.dir != dir || s.prompt_hash != hash || s.child.try_wait().ok().flatten().is_some());
    if stale {
        if let Some(mut old) = guard.take() {
            kill(&mut old);
        }
        *guard = Some(spawn(app, &dir, &system_path, hash, Arc::clone(&chat.busy))?);
        chat.library_seen.store(0, std::sync::atomic::Ordering::SeqCst);
    }
    let session = guard.as_mut().ok_or_else(|| AppError::new("internal", "no session"))?;
    let library = prompt::library_block(app, &chat.library_seen);
    let content = format!("{}{library}{}", prompt::turn_context(&doc), text);
    let line = serde_json::json!({ "type": "user", "message": { "role": "user", "content": content } });
    writeln!(session.stdin, "{line}").and_then(|()| session.stdin.flush()).map_err(|e| AppError::new("claudeStart", e.to_string()))?;
    chat.busy.store(true, std::sync::atomic::Ordering::SeqCst);
    log_item(&dir, if kind == "decision" { "decision" } else { "user" }, text);
    // Новый ход: правки Claude в нем подписываются `claude#N` (отмена хода одним действием).
    if let Some(mcp) = app.try_state::<crate::mcp::McpState>() {
        mcp.turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(())
}

/// Прервать ответ (процесс останавливается; следующее сообщение продолжит ту же сессию).
#[tauri::command]
#[specta::specta]
pub fn chat_stop(chat: State<'_, ChatState>) {
    shutdown(&chat);
}

/// Начать новый разговор: прошлая история — в архив проекта.
#[tauri::command]
#[specta::specta]
pub fn chat_new_session(chat: State<'_, ChatState>, project: State<'_, ProjectState>) -> AppResult<()> {
    shutdown(&chat);
    let (dir, _) = project.context().ok_or_else(|| AppError::new("noProject", "no project"))?;
    let cdir = claude_dir(&dir);
    let _ = std::fs::remove_file(cdir.join("session.json"));
    let log = cdir.join("chat.jsonl");
    if log.is_file() {
        std::fs::rename(&log, cdir.join(format!("chat-{}.jsonl", now())))?;
    }
    Ok(())
}

/// История чата открытого проекта (последние строки).
#[tauri::command]
#[specta::specta]
pub fn chat_history(project: State<'_, ProjectState>) -> Vec<ChatItem> {
    let Some((dir, _)) = project.context() else { return vec![] };
    let Ok(raw) = std::fs::read_to_string(claude_dir(&dir).join("chat.jsonl")) else { return vec![] };
    let items: Vec<ChatItem> = raw.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    items.into_iter().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect()
}

impl ChatState {
    pub fn is_busy(&self) -> bool {
        self.busy.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Идет ли ответ сейчас.
#[tauri::command]
#[specta::specta]
pub fn chat_busy(chat: State<'_, ChatState>) -> bool {
    chat.busy.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{NewProject, Preset, create_on_disk};

    /// Живой ход в настоящем `claude` (подписка, ~0.01 $ оценки): правка правил разрешена, правка
    /// вне `rules/` — запрещена. `cargo test live_chat_turn -- --ignored --nocapture`.
    #[test]
    #[ignore = "живой запрос к Claude"]
    fn live_chat_turn() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, _) = create_on_disk(&NewProject { name: "Live".into(), parent: tmp.path().display().to_string(), preset: Preset::Vertical, fps: 60 }).unwrap();
        let doc = crate::project::load(&dir).unwrap();
        let cdir = claude_dir(&dir);
        std::fs::create_dir_all(&cdir).unwrap();
        let (system, _) = prompt::system_prompt(&dir, &doc, "test");
        std::fs::write(cdir.join("system.md"), system).unwrap();
        std::fs::write(cdir.join("mcp.json"), r#"{"mcpServers":{}}"#).unwrap();
        let claude = which::which("claude").unwrap();
        let mut cmd = build_command(&claude, &dir, &cdir.join("system.md"), &cdir.join("mcp.json"), None);
        cmd.args(["--model", "haiku"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let msg = "Do two things with tools, no questions: 1) write the single line `Calm promo, 30 s.` into rules/brief.md; 2) create a file notes.txt in the project root with the word test, using Write. Then reply `done`.";
        let line = serde_json::json!({ "type": "user", "message": { "role": "user", "content": format!("{}{msg}", prompt::turn_context(&doc)) } });
        writeln!(child.stdin.take().unwrap(), "{line}").unwrap();
        let mut parser = events::Parser::default();
        let mut turn = None;
        for l in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            for p in parser.feed(&l, now()) {
                if let Parsed::Event(e) = p {
                    println!("{e:?}");
                    if matches!(e, ChatEvent::Turn { .. }) {
                        turn = Some(e);
                    }
                }
            }
            if turn.is_some() {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let brief = std::fs::read_to_string(dir.join("rules/brief.md")).unwrap();
        assert!(brief.contains("Calm promo"), "правила проекта правятся: {brief:?}");
        assert!(!dir.join("notes.txt").exists(), "вне rules/ запись запрещена");
        let Some(ChatEvent::Turn { denials, .. }) = turn else { panic!("нет итога хода") };
        assert!(denials.iter().any(|d| d.starts_with("Write")), "запрет виден в итоге хода: {denials:?}");
    }
}
