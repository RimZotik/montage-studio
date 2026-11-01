//! Разбор потока событий `claude` (`--output-format stream-json`) в события чата. Формат снят с
//! живого вывода 2.1.316: `system/init` (сессия, модель), `stream_event` с `text_delta`
//! (текст по частям), `assistant` (готовое сообщение: текст и вызовы инструментов), `user` с
//! `tool_result`, `rate_limit_event`, `result` (итог хода).

use serde::Serialize;
use serde_json::Value;
use specta::Type;

use crate::claude_limits::{Limits, parse_event};

#[derive(Debug, Clone, Serialize, Type, PartialEq, tauri_specta::Event)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum ChatEvent {
    /// Модель ответила впервые в процессе.
    Ready { model: String },
    /// Кусок текста ответа (печатается по мере прихода).
    Delta { text: String },
    /// Готовый текст сообщения (заменяет собранные куски).
    Assistant { text: String },
    /// Вызов инструмента: имя и коротко, над чем.
    Tool { id: String, name: String, summary: String },
    ToolDone { id: String, error: bool },
    /// Итог хода.
    Turn {
        ok: bool,
        interrupted: bool,
        #[specta(type = specta_typescript::Number)]
        duration_ms: u64,
        #[specta(type = specta_typescript::Number)]
        input_tokens: u64,
        #[specta(type = specta_typescript::Number)]
        output_tokens: u64,
        cost_usd: f64,
        /// Запрещенные действия (инструмент и над чем).
        denials: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnUsage {
    pub session: String,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Event(ChatEvent),
    Session { id: String, model: String },
    Limits(Limits),
    Usage(TurnUsage),
}

#[derive(Default)]
pub struct Parser {
    model: String,
    session: String,
    /// Стоимость сессии до этого хода (`total_cost_usd` накопительный).
    cost_before: f64,
    /// Папка проекта (из `init`, через `/`).
    root: String,
}

fn u(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// Над чем работает инструмент: путь (относительно папки проекта `root`), шаблон или команда.
pub fn summary(input: &Value, root: &str) -> String {
    if let Some(ops) = input["ops"].as_array() {
        return format!("{} × {}", ops.len(), input["label"].as_str().unwrap_or("ops"));
    }
    if let Some(f) = input["frame"].as_u64() {
        return format!("{f}");
    }
    for key in ["file_path", "path", "pattern", "command", "url"] {
        if let Some(s) = input[key].as_str() {
            let s = s.replace('\\', "/");
            let short = if root.is_empty() { s.as_str() } else { s.strip_prefix(root).map_or(s.as_str(), |r| r.trim_start_matches('/')) };
            let short = if short.is_empty() { "." } else { short };
            return short.chars().take(120).collect();
        }
    }
    String::new()
}

impl Parser {
    pub fn feed(&mut self, line: &str, now: i64) -> Vec<Parsed> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
        let mut out = Vec::new();
        match v["type"].as_str().unwrap_or("") {
            "system" if v["subtype"] == "init" => {
                self.session = v["session_id"].as_str().unwrap_or_default().to_owned();
                self.model = v["model"].as_str().unwrap_or_default().to_owned();
                self.root = v["cwd"].as_str().unwrap_or_default().replace('\\', "/");
                out.push(Parsed::Session { id: self.session.clone(), model: self.model.clone() });
                out.push(Parsed::Event(ChatEvent::Ready { model: self.model.clone() }));
            }
            "stream_event" => {
                let e = &v["event"];
                if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta"
                    && let Some(t) = e["delta"]["text"].as_str()
                {
                    out.push(Parsed::Event(ChatEvent::Delta { text: t.to_owned() }));
                }
            }
            "assistant" => {
                let mut text = String::new();
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => text.push_str(block["text"].as_str().unwrap_or_default()),
                        Some("tool_use") => out.push(Parsed::Event(ChatEvent::Tool {
                            id: block["id"].as_str().unwrap_or_default().to_owned(),
                            name: block["name"].as_str().unwrap_or_default().to_owned(),
                            summary: summary(&block["input"], &self.root),
                        })),
                        _ => {}
                    }
                }
                if !text.is_empty() {
                    out.insert(0, Parsed::Event(ChatEvent::Assistant { text }));
                }
            }
            "user" => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" {
                        out.push(Parsed::Event(ChatEvent::ToolDone { id: block["tool_use_id"].as_str().unwrap_or_default().to_owned(), error: block["is_error"].as_bool().unwrap_or(false) }));
                    }
                }
            }
            "rate_limit_event" => {
                if let Some(l) = parse_event(line, now) {
                    out.push(Parsed::Limits(l));
                }
            }
            "result" => {
                let usage = &v["usage"];
                let total = v["total_cost_usd"].as_f64().unwrap_or(0.0);
                let cost = (total - self.cost_before).max(0.0);
                self.cost_before = total;
                let denials = v["permission_denials"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|d| format!("{} {}", d["tool_name"].as_str().unwrap_or("?"), summary(&d["tool_input"], &self.root)).trim().to_owned())
                    .collect();
                let (input, output) = (u(&usage["input_tokens"]) + u(&usage["cache_creation_input_tokens"]) + u(&usage["cache_read_input_tokens"]), u(&usage["output_tokens"]));
                out.push(Parsed::Usage(TurnUsage {
                    session: self.session.clone(),
                    model: self.model.clone(),
                    input: u(&usage["input_tokens"]),
                    output,
                    cache_read: u(&usage["cache_read_input_tokens"]),
                    cache_create: u(&usage["cache_creation_input_tokens"]),
                    cost_usd: cost,
                    duration_ms: u(&v["duration_ms"]),
                }));
                out.push(Parsed::Event(ChatEvent::Turn { ok: v["is_error"] != true, interrupted: false, duration_ms: u(&v["duration_ms"]), input_tokens: input, output_tokens: output, cost_usd: cost, denials }));
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(p: &mut Parser, line: &str) -> Vec<ChatEvent> {
        p.feed(line, 1).into_iter().filter_map(|x| if let Parsed::Event(e) = x { Some(e) } else { None }).collect()
    }

    #[test]
    fn stream_shapes() {
        let mut p = Parser::default();
        assert_eq!(events(&mut p, r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5","cwd":"D:\\p"}"#), vec![ChatEvent::Ready { model: "claude-haiku-5-5".into() }]);
        assert_eq!(events(&mut p, r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hi"}}}"#), vec![ChatEvent::Delta { text: "Hi".into() }]);
        assert!(events(&mut p, r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"x"}}}"#).is_empty());
        let a = events(&mut p, r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"Done"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"D:\\p\\rules\\brief.md"}}]}}"#);
        assert_eq!(a, vec![ChatEvent::Assistant { text: "Done".into() }, ChatEvent::Tool { id: "t1".into(), name: "Read".into(), summary: "rules/brief.md".into() }]);
        assert_eq!(events(&mut p, r#"{"type":"user","message":{"content":[{"tool_use_id":"t1","type":"tool_result","content":"x"}]}}"#), vec![ChatEvent::ToolDone { id: "t1".into(), error: false }]);
    }

    #[test]
    fn result_cost_is_per_turn() {
        let mut p = Parser::default();
        let r1 = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1500,"total_cost_usd":0.01,"usage":{"input_tokens":2,"cache_creation_input_tokens":10,"cache_read_input_tokens":20,"output_tokens":5},"permission_denials":[{"tool_name":"Edit","tool_input":{"file_path":"project.json"}}]}"#;
        let e = events(&mut p, r1);
        assert_eq!(e, vec![ChatEvent::Turn { ok: true, interrupted: false, duration_ms: 1500, input_tokens: 32, output_tokens: 5, cost_usd: 0.01, denials: vec!["Edit project.json".into()] }]);
        let r2 = r1.replace("0.01", "0.025");
        let ChatEvent::Turn { cost_usd, .. } = events(&mut p, &r2).pop().unwrap() else { panic!() };
        assert!((cost_usd - 0.015).abs() < 1e-9, "накопительная стоимость → стоимость хода");
    }
}
