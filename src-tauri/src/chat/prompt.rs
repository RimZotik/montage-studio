//! Постоянный контекст Claude (`--append-system-prompt-file`) и блок контекста хода
//! (`docs/assistant-layer.md`): правила студии (встроены в программу), возможности этой версии,
//! правила проекта, последовательность. Хэш содержимого — чтобы перезапускать процесс, только
//! когда контекст изменился.

use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::project::model::Snapshot;

/// Правила студии — внутри программы, пользователь их не видит и не правит.
const CORE_RULES: &str = include_str!("../../resources/claude/core-rules.md");
/// Файлы правил проекта по порядку.
const PROJECT_RULES: [&str; 4] = ["brief", "rules", "style", "script"];
/// Правило длиннее — обрезается (защита от случайно вставленного огромного текста).
const MAX_RULE_CHARS: usize = 20_000;

/// Что умеет студия этой версии — чтобы Claude не обещал недоступного.
fn capabilities(version: &str) -> String {
    format!(
        "## This studio version ({version})\n\
         - Built-in tools: Read, Glob, Grep; Edit/Write only `rules/*.md`.\n\
         - Studio MCP tools (server `studio`): `project_summary`, `media_list`, `timeline_get`, `timeline_apply` (batched ops, one undo step), \
         `ui_seek`, `ui_select`, `ui_tab`, `log_errors`. Start with `project_summary`; read only the time range you need with `timeline_get`; \
         put all edits of one step into one `timeline_apply` call. Times are frames (seconds × fps).\n\
         - Library and sound tools: `library_list`, `library_get`, `library_save`, `pack_edit`, `effect_apply`, `tool_run`, `speech_transcribe`, `speech_find`, `audio_analyze`, `audio_peaks`, `render_frame`, `export_start`, `export_status`. See the rules below.\n\
         - Not available yet: code-driven effects and smart clips — plan them and say so.\n\
         - Proposals and questions use fenced blocks the chat renders as cards (instead of `ui.propose` / `ui.ask`):\n\
         ```studio-proposal\n{{\"id\":\"p1\",\"kind\":\"plan|tool|effect|smart|rule|project\",\"title\":\"…\",\"description\":\"…\",\"details\":[\"…\"]}}\n```\n\
         ```studio-ask\n{{\"id\":\"q1\",\"question\":\"…\",\"options\":[\"…\",\"…\"]}}\n```\n\
         The user's answer arrives as `[proposal p1: accepted]`, `[proposal p1: rejected]`, `[proposal p1: edited] …` or `[answer q1] …`.\n"
    )
}

/// Собрать постоянный контекст и его хэш.
pub fn system_prompt(dir: &Path, doc: &Snapshot, version: &str) -> (String, u64) {
    let mut out = String::new();
    out.push_str(CORE_RULES);
    out.push('\n');
    out.push_str(&capabilities(version));
    let seq = &doc.project.sequence;
    out.push_str(&format!(
        "\n## Project\n- Name: {}\n- Sequence: {}x{} at {} fps, background {}\n",
        doc.project.name, seq.width, seq.height, seq.fps, seq.background
    ));
    out.push_str("\n## Project rules (written by the user; follow them)\n");
    let mut any = false;
    for name in PROJECT_RULES {
        let text = std::fs::read_to_string(dir.join("rules").join(format!("{name}.md"))).unwrap_or_default();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        any = true;
        let cut: String = text.chars().take(MAX_RULE_CHARS).collect();
        out.push_str(&format!("\n### rules/{name}.md\n{cut}\n"));
    }
    if !any {
        out.push_str("(none yet — offer to write a brief together if the task needs one)\n");
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    out.hash(&mut h);
    (out, h.finish())
}

/// Блок контекста перед сообщением пользователя (позже — выделение и точка воспроизведения).
pub fn turn_context(doc: &Snapshot) -> String {
    let seq = &doc.project.sequence;
    format!("[studio: {}x{} {}fps]\n", seq.width, seq.height, seq.fps)
}

/// Короткий список библиотеки: только если изменился с прошлого раза (экономия токенов — Claude
/// знает, что уже есть, и не придумывает заново).
pub fn library_block(app: &tauri::AppHandle, seen: &std::sync::atomic::AtomicU64) -> String {
    use tauri::Manager;
    let Ok(root) = crate::library::root_of(app) else { return String::new() };
    if crate::library::init(&root).is_err() {
        return String::new();
    }
    let used = app.try_state::<crate::db::Db>().and_then(|db| crate::library::usage(&db).ok()).unwrap_or_default();
    // Хэш — по составу, а не по счетчикам: порядок от использования не повод отправлять заново.
    let mut names: Vec<String> = crate::library::items(&root).iter().map(|(i, _)| format!("{}#{}", i.id, i.revision)).collect();
    names.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    names.hash(&mut h);
    let hash = h.finish();
    if seen.swap(hash, std::sync::atomic::Ordering::SeqCst) == hash {
        return String::new();
    }
    crate::library::compact_index(&root, &used, 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{NewProject, Preset, create_on_disk};

    #[test]
    fn rules_change_changes_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, _) = create_on_disk(&NewProject { name: "P".into(), parent: tmp.path().display().to_string(), preset: Preset::Vertical, fps: 60 }).unwrap();
        let doc = crate::project::load(&dir).unwrap();
        let (a, ha) = system_prompt(&dir, &doc, "0.4.0");
        assert!(a.contains("Studio core rules") && a.contains("none yet"));
        std::fs::write(dir.join("rules/brief.md"), "Short promo, calm tone.").unwrap();
        let (b, hb) = system_prompt(&dir, &doc, "0.4.0");
        assert!(b.contains("### rules/brief.md\nShort promo, calm tone."));
        assert_ne!(ha, hb);
    }
}
