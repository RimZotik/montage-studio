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
         - Available now: reading project files (Read, Glob, Grep) and editing `rules/*.md` (Edit, Write).\n\
         - Not available yet: studio MCP tools (timeline, captions, audio, smart clips, render). Do not pretend to edit the timeline; \
         when the user asks for it, plan the edit precisely (clips, times, effects) and say it will be applied once timeline tools arrive.\n\
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
