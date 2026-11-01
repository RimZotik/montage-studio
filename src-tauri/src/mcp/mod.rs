//! MCP-сервер студии для Claude (`docs/claude-bridge.md`): HTTP на 127.0.0.1 со случайными портом
//! и ключом на каждый запуск, JSON-RPC (`initialize`, `tools/list`, `tools/call`). Инструменты
//! (`tools.rs`) работают с проектом через сервис проекта и с интерфейсом через события — тем же
//! путем, что и действия пользователя. Ответы короткие и текстовые — экономия токенов.

mod dev;
mod studio;
mod tools;

use std::io::Read;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Value, json};
use specta::Type;
use tauri::AppHandle;

/// Адрес и ключ сервера (для `mcp.json` чата).
#[derive(Debug, Clone)]
pub struct McpInfo {
    pub url: String,
    pub token: String,
    /// Ключ разработчика: те же инструменты и `dev_*` (только в `mcp-dev.json`).
    pub dev_token: String,
}

/// Что интерфейс сообщил о себе (головка, выделение, вкладка) — для `project_summary`.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub frame: u32,
    pub selected: Vec<String>,
    pub tab: String,
}

#[derive(Default)]
pub struct McpState {
    pub info: Mutex<Option<McpInfo>>,
    pub ui: Mutex<UiState>,
    /// Номер хода Claude: правки хода подписываются `claude#N` (отмена хода одним действием).
    pub turn: std::sync::atomic::AtomicU64,
}

impl McpState {
    pub fn info(&self) -> Option<McpInfo> {
        self.info.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }
}

/// Команда интерфейсу от Claude (перейти к кадру, выделить, вкладка, масштаб).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum UiCommand {
    Seek { frame: u32 },
    Select { ids: Vec<String> },
    Tab { tab: String },
    /// Claude правит область (`timeline`, `rules`) — интерфейс показывает блокировку до конца хода.
    Busy { area: String },
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    // Два независимых случайных 64-битных числа от системного генератора хэшей.
    let a = std::collections::hash_map::RandomState::new().build_hasher().finish();
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(a ^ std::process::id() as u64);
    format!("{a:016x}{:016x}", h.finish())
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Обработать одно сообщение JSON-RPC (`None` — уведомление, ответа нет). `extra` — список
/// дополнительных инструментов (разработчика), `extra_call` — их вызов (`None` — не его).
fn handle_with<R: tauri::Runtime>(app: &AppHandle<R>, msg: &Value, extra_call: impl Fn(&str, &Value) -> Option<Result<String, String>>, extra: Option<Value>) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    Some(match method {
        "initialize" => reply(
            &id,
            json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "montage-studio", "version": env!("CARGO_PKG_VERSION") },
            }),
        ),
        "ping" => reply(&id, json!({})),
        "tools/list" => {
            let mut all = tools::list();
            if let Some(list) = all.as_array_mut() {
                list.extend(studio::list().as_array().into_iter().flatten().cloned());
            }
            if let (Some(list), Some(more)) = (all.as_array_mut(), extra.as_ref().and_then(Value::as_array)) {
                list.extend(more.iter().cloned());
            }
            reply(&id, json!({ "tools": all }))
        }
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"]["arguments"].clone();
            let result = extra_call(name, &args).unwrap_or_else(|| tools::call(app, name, &args));
            let (text, is_error) = match result {
                Ok(t) => (t, false),
                Err(e) => (e, true),
            };
            log::info!("mcp tool {name}: {}", if is_error { "error" } else { "ok" });
            reply(&id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        _ => error(&id, -32601, "method not found"),
    })
}

/// Запустить сервер в отдельном потоке.
/// Запустить сервер без инструментов разработчика (тесты с тестовым приложением).
#[cfg(test)]
pub fn start<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<McpInfo> {
    start_with(app, |_, _, _| None)
}

/// Сервер приложения: с инструментами разработчика по их ключу.
pub fn start_app(app: &AppHandle) -> Option<McpInfo> {
    let dev_app = app.clone();
    start_with(app, move |name, args, dev| {
        if dev && name.starts_with("dev_") {
            Some(dev::call(&dev_app, name, args))
        } else {
            studio::call(&dev_app, name, args)
        }
    })
}

fn start_with<R: tauri::Runtime>(app: &AppHandle<R>, extra: impl Fn(&str, &Value, bool) -> Option<Result<String, String>> + Send + Sync + 'static) -> Option<McpInfo> {
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| log::warn!("mcp server: {e}")).ok()?;
    let port = server.server_addr().to_ip().map(|a| a.port())?;
    let info = McpInfo { url: format!("http://127.0.0.1:{port}/mcp"), token: random_token(), dev_token: random_token() };
    let (app2, token, dev_token) = (app.clone(), info.token.clone(), info.dev_token.clone());
    let extra = std::sync::Arc::new(extra);
    std::thread::spawn(move || {
        for mut req in server.incoming_requests() {
            let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_owned()).unwrap_or_default();
            let dev = auth == format!("Bearer {dev_token}");
            let authorized = dev || auth == format!("Bearer {token}");
            if !authorized {
                let _ = req.respond(tiny_http::Response::empty(401));
                continue;
            }
            if req.method() != &tiny_http::Method::Post {
                let _ = req.respond(tiny_http::Response::empty(405));
                continue;
            }
            let mut body = String::new();
            if req.as_reader().take(4 << 20).read_to_string(&mut body).is_err() {
                let _ = req.respond(tiny_http::Response::empty(400));
                continue;
            }
            let Ok(msg) = serde_json::from_str::<Value>(&body) else {
                let _ = req.respond(tiny_http::Response::empty(400));
                continue;
            };
            // Свой поток: долгий вызов (анализ, ожидание ответа) не держит остальные.
            let (app3, extra2) = (app2.clone(), std::sync::Arc::clone(&extra));
            std::thread::spawn(move || {
                // Ошибка в одном вызове не останавливает сервер.
                let dev_list = dev.then(dev::list);
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle_with(&app3, &msg, |n, a| extra2(n, a, dev), dev_list)))
                    .unwrap_or_else(|_| msg.get("id").map(|id| error(id, -32603, "internal error")));
                let response = match out {
                    Some(out) => {
                        let header = tiny_http::Header::from_bytes("Content-Type", "application/json").ok();
                        let mut r = tiny_http::Response::from_string(out.to_string());
                        if let Some(h) = header {
                            r = r.with_header(h);
                        }
                        req.respond(r)
                    }
                    None => req.respond(tiny_http::Response::empty(202)),
                };
                if let Err(e) = response {
                    log::info!("mcp respond: {e}");
                }
            });
        }
    });
    log::info!("mcp server on 127.0.0.1:{port}");
    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_is_valid() {
        let list = tools::list();
        let names: Vec<&str> = list.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"timeline_apply") && names.contains(&"project_summary"));
        for t in list.as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
            assert!(t["description"].as_str().unwrap().len() < 1200, "описание короткое (токены)");
        }
        assert_eq!(random_token().len(), 32);
    }

    /// Сквозная проверка: настоящий `claude` через MCP студии ставит клип на таймлайн, правка — в
    /// истории как ход Claude. `cargo test live_claude_edits_timeline -- --ignored --nocapture`.
    #[test]
    #[ignore = "живой запрос к Claude"]
    fn live_claude_edits_timeline() {
        use std::io::{BufRead, BufReader, Write};
        use tauri::Manager;
        let app = tauri::test::mock_builder().manage(crate::project::ProjectState::default()).manage(McpState::default()).build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let h = app.handle().clone();
        let tmp = tempfile::tempdir().unwrap();
        let (dir, _) = crate::projects::create_on_disk(&crate::projects::NewProject { name: "Live".into(), parent: tmp.path().display().to_string(), preset: crate::projects::Preset::Vertical, fps: 60 }).unwrap();
        std::fs::write(dir.join("media/footage/intro.mp4"), b"fake").unwrap();
        let doc = crate::project::load(&dir).unwrap();
        h.state::<crate::project::ProjectState>().open_for_test(dir.clone(), doc.clone());
        let info = start(&h).expect("mcp server");
        let cdir = dir.join(".studio/claude");
        std::fs::create_dir_all(&cdir).unwrap();
        let (system, _) = crate::chat::prompt::system_prompt(&dir, &doc, "test");
        std::fs::write(cdir.join("system.md"), system).unwrap();
        let mcp = serde_json::json!({ "mcpServers": { "studio": { "type": "http", "url": info.url, "headers": { "Authorization": format!("Bearer {}", info.token) } } } });
        std::fs::write(cdir.join("mcp.json"), mcp.to_string()).unwrap();
        let claude = which::which("claude").unwrap();
        let mut cmd = crate::chat::build_command(&claude, &dir, &cdir.join("system.md"), &cdir.join("mcp.json"), None);
        cmd.args(["--model", "haiku"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let msg = "Put media/footage/intro.mp4 on track v1 from frame 0, 120 frames long, then add a caption \"Hello\" on s1 from frame 30 for 60 frames. Use the studio tools, no questions.";
        let line = serde_json::json!({ "type": "user", "message": { "role": "user", "content": msg } });
        writeln!(child.stdin.take().unwrap(), "{line}").unwrap();
        for l in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
            if l.contains("\"tool_use\"") || l.contains("\"type\":\"result\"") {
                println!("{}", &l[..l.len().min(300)]);
            }
            if l.contains("\"type\":\"result\"") {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let (_, now) = h.state::<crate::project::ProjectState>().context().unwrap();
        let clips = &now.timeline.clips;
        println!("clips: {clips:?}");
        assert!(clips.iter().any(|c| c.track == "v1" && c.src.as_deref() == Some("media/footage/intro.mp4") && c.duration == 120), "клип поставлен");
        assert!(clips.iter().any(|c| c.track == "s1" && c.text.as_deref() == Some("Hello")), "субтитр поставлен");
        assert!(h.state::<crate::project::ProjectState>().undo_label().is_some_and(|l| l.starts_with("claude#")), "ход Claude в истории");
        let disk = crate::project::load(&dir).unwrap();
        assert_eq!(disk.timeline.clips.len(), clips.len(), "записано на диск");
    }
}

/// Интерфейс сообщает головку, выделение и вкладку (для сводки Claude).
#[tauri::command]
#[specta::specta]
pub fn ui_state_report(state: tauri::State<'_, McpState>, ui: UiState) {
    *state.ui.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = ui;
}
