//! Инструменты MCP для библиотеки, речи и анализа звука. Долгие вызовы ждут ограниченное время:
//! клиент MCP не должен висеть — незаконченное отвечает «еще идет», вызов повторяется позже.

use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use super::McpState;
use crate::audio::{self, Band};
use crate::checks::{ToolId, find};
use crate::library::{self, Item};
use crate::project::ProjectState;
use crate::{presets, speech};

/// Сколько инструмент ждет расшифровку, прежде чем ответить «еще идет».
const WAIT: Duration = Duration::from_secs(50);

pub fn list() -> Value {
    json!([
        { "name": "library_list", "description": "Effects and tools in the library. Optional kind (effect|tool), query (text), pack. One line each: id, targets, controls, uses.", "inputSchema": { "type": "object", "properties": { "kind": { "type": "string" }, "query": { "type": "string" }, "pack": { "type": "string" } } } },
        { "name": "library_get", "description": "Full JSON of a library item (before changing it).", "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] } },
        { "name": "library_save", "description": "Create or update an effect or tool, including its control panel. New items go to `pack` (default main). The same id updates it and keeps history. Format: see core rules.", "inputSchema": { "type": "object", "properties": { "item": { "type": "object" }, "pack": { "type": "string" } }, "required": ["item"] } },
        { "name": "pack_edit", "description": "Packs. action: list | create (name) | add (pack,item) | remove (pack,item) | delete (pack; its items move to main).", "inputSchema": { "type": "object", "properties": { "action": { "type": "string" }, "pack": { "type": "string" }, "item": { "type": "string" }, "name": { "type": "string" } }, "required": ["action"] } },
        { "name": "effect_apply", "description": "Put a library effect on clips (default: selection). params override its defaults. One undo step. Returns effect ids on the clips.", "inputSchema": { "type": "object", "properties": { "id": { "type": "string" }, "clips": { "type": "array", "items": { "type": "string" } }, "params": { "type": "object" } }, "required": ["id"] } },
        { "name": "tool_run", "description": "Run a library tool on clips (default: selection, else its track settings). params override its defaults. One undo step. Status pending: run again later.", "inputSchema": { "type": "object", "properties": { "id": { "type": "string" }, "clips": { "type": "array", "items": { "type": "string" } }, "params": { "type": "object" } }, "required": ["id"] } },
        { "name": "style_apply", "description": "Put a library style (rules for how the video should be edited) into the project rules/style.md; they join your context from the next message.", "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] } },
        { "name": "sound_add", "description": "Put a library sound on the timeline so that its hit lands on a frame (synthesized once into media/sfx). track default: last audio track; volume 0–2.", "inputSchema": { "type": "object", "properties": { "id": { "type": "string" }, "frame": { "type": "integer" }, "track": { "type": "string" }, "volume": { "type": "number" } }, "required": ["id", "frame"] } },
        { "name": "speech_transcribe", "description": "What is said in a media file or on a clip, phrase by phrase (local model, cached). path — seconds in the file; clip — timeline frames, only what is audible. Status pending: call again.", "inputSchema": { "type": "object", "properties": { "path": { "type": "string" }, "clip": { "type": "string" }, "language": { "type": "string" } } } },
        { "name": "speech_find", "description": "Find words in already transcribed clips: timeline frames where they are said.", "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] } },
        { "name": "audio_analyze", "description": "Loudness (LUFS, peak, range) and pauses of a media file.", "inputSchema": { "type": "object", "properties": { "path": { "type": "string" }, "threshold": { "type": "number" }, "min_pause": { "type": "number" } }, "required": ["path"] } },
        { "name": "render_frame", "description": "Render one timeline frame to a JPEG (exactly as export will look) and return its path: read it with Read to see the result. Use sparingly, one frame at a time.", "inputSchema": { "type": "object", "properties": { "frame": { "type": "integer" }, "scale": { "type": "number" } }, "required": ["frame"] } },
        { "name": "export_start", "description": "Render the video to output/ in the background. quality: high (default, for social networks) | max | compact. Only when the user asks for it. Check with export_status.", "inputSchema": { "type": "object", "properties": { "name": { "type": "string" }, "quality": { "type": "string" } } } },
        { "name": "export_status", "description": "Is the render engine installed, is an export running, finished files with size and bitrate.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "audio_peaks", "description": "Beats and hits of a media file, seconds. band: bass|full; sensitivity 0–100; min_gap seconds.", "inputSchema": { "type": "object", "properties": { "path": { "type": "string" }, "band": { "type": "string" }, "sensitivity": { "type": "number" }, "min_gap": { "type": "number" } }, "required": ["path"] } },
    ])
}

fn text_of(v: &Value) -> String {
    v.as_str().map_or_else(|| v.get("en").or_else(|| v.get("ru")).and_then(Value::as_str).unwrap_or("").to_owned(), str::to_owned)
}

fn clips_or_selection(app: &AppHandle, args: &Value) -> Vec<String> {
    match serde_json::from_value::<Vec<String>>(args["clips"].clone()) {
        Ok(ids) if !ids.is_empty() => ids,
        _ => app.state::<McpState>().ui.lock().unwrap_or_else(std::sync::PoisonError::into_inner).selected.clone(),
    }
}

fn turn_label(app: &AppHandle) -> String {
    format!("claude#{}", app.state::<McpState>().turn.load(std::sync::atomic::Ordering::SeqCst))
}

fn media_file(app: &AppHandle, path: &str) -> Result<std::path::PathBuf, String> {
    let (dir, _) = app.state::<ProjectState>().context().ok_or("no project is open")?;
    if !path.starts_with("media/") || path.contains("..") {
        return Err("path must be inside media/".into());
    }
    Ok(dir.join(path))
}

fn ffmpeg(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    find(app, ToolId::Ffmpeg).ok_or_else(|| "ffmpeg is not installed".to_owned())
}

/// Инструменты этого файла; чужое имя — `None`.
pub fn call(app: &AppHandle, name: &str, args: &Value) -> Option<Result<String, String>> {
    let run = || -> Result<String, String> {
        let root = library::root_of(app).map_err(|e| e.message)?;
        library::init(&root).map_err(|e| e.message)?;
        match name {
            "library_list" => {
                let used = library::usage(&app.state::<crate::db::Db>()).unwrap_or_default();
                let (kind, pack, query) = (args["kind"].as_str(), args["pack"].as_str(), args["query"].as_str().map(str::to_lowercase));
                let packs = library::packs(&root);
                let mut lines: Vec<String> = library::items(&root)
                    .into_iter()
                    .filter(|(i, owner)| kind.is_none_or(|k| k == i.kind) && pack.is_none_or(|p| p == owner || packs.iter().any(|x| x.id == p && x.refs.contains(&i.id))))
                    .filter(|(i, _)| query.as_ref().is_none_or(|q| format!("{} {} {} {}", i.id, i.name, i.description, i.tags.join(" ")).to_lowercase().contains(q)))
                    .map(|(i, owner)| {
                        let ids: Vec<&str> = i.controls.as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).collect();
                        let n = used.get(&i.id).map_or(0, |u| u.0);
                        format!("{} {} [{}] ({}) {} — {} pack={owner} uses={n}", i.id, i.kind, i.targets.join(","), ids.join(","), text_of(&i.name), text_of(&i.description))
                    })
                    .collect();
                lines.sort();
                Ok(if lines.is_empty() { "nothing found".into() } else { lines.join("\n") })
            }
            "library_get" => {
                let id = args["id"].as_str().ok_or("id required")?;
                let (item, pack) = library::find(&root, id).ok_or_else(|| format!("{id} not found"))?;
                Ok(format!("pack={pack}\n{}", serde_json::to_string(&item).map_err(|e| e.to_string())?))
            }
            "library_save" => {
                let mut raw = args["item"].clone();
                if raw.get("author").is_none() {
                    raw["author"] = json!("claude");
                }
                let item: Item = serde_json::from_value(raw).map_err(|e| format!("bad item: {e}"))?;
                let saved = library::save(&root, item, args["pack"].as_str()).map_err(|e| format!("refused ({}): {}", e.code, e.message))?;
                library::changed(app);
                Ok(format!("saved {} revision {}", saved.id, saved.revision))
            }
            "pack_edit" => {
                let action = args["action"].as_str().ok_or("action required")?;
                let pack = args["pack"].as_str().unwrap_or("");
                let item = args["item"].as_str().unwrap_or("");
                let out = match action {
                    "list" => {
                        let all = library::items(&root);
                        Ok(library::packs(&root).iter().map(|p| format!("{} {} own={} refs={}{}", p.id, text_of(&p.name), all.iter().filter(|(_, o)| o == &p.id).count(), p.refs.len(), if p.readonly { " read-only" } else { "" })).collect::<Vec<_>>().join("\n"))
                    }
                    "create" => library::pack_create(&root, args["name"].as_str().ok_or("name required")?, "").map(|p| format!("created {}", p.id)),
                    "add" => library::pack_link(&root, pack, item, true).map(|()| "ok".to_owned()),
                    "remove" => library::pack_link(&root, pack, item, false).map(|()| "ok".to_owned()),
                    "delete" => library::pack_delete(&root, pack).map(|()| "ok".to_owned()),
                    other => return Err(format!("unknown action {other}")),
                }
                .map_err(|e| format!("refused ({}): {}", e.code, e.message))?;
                library::changed(app);
                Ok(out)
            }
            "effect_apply" => {
                let id = args["id"].as_str().ok_or("id required")?;
                let clips = clips_or_selection(app, args);
                let made = presets::apply_effect(app, id, &args["params"], &clips, &turn_label(app)).map_err(|e| format!("refused ({}): {}", e.code, e.message))?;
                Ok(format!("ok: {}", clips.iter().zip(&made).map(|(c, f)| format!("{c}/{f}")).collect::<Vec<_>>().join(",")))
            }
            "tool_run" => {
                let id = args["id"].as_str().ok_or("id required")?;
                let r = presets::run(app, id, &args["params"], &clips_or_selection(app, args), &turn_label(app), WAIT).map_err(|e| format!("refused ({}): {}", e.code, e.message))?;
                Ok(format!("{}: {}", r.status, r.note))
            }
            "style_apply" => {
                let id = args["id"].as_str().ok_or("id required")?;
                presets::style_apply_to_project(app, id, "ru").map(|p| format!("ok: {p}")).map_err(|e| format!("refused ({}): {}", e.code, e.message))
            }
            "sound_add" => {
                let id = args["id"].as_str().ok_or("id required")?;
                let frame = args["frame"].as_u64().and_then(|f| u32::try_from(f).ok()).ok_or("frame required")?;
                presets::sound_add(app, id, frame, args["track"].as_str(), args["volume"].as_f64(), &turn_label(app)).map(|c| format!("ok: {c}")).map_err(|e| format!("refused ({}): {}", e.code, e.message))
            }
            "speech_transcribe" => {
                let (dir, doc) = app.state::<ProjectState>().context().ok_or("no project is open")?;
                let clip = args["clip"].as_str().and_then(|id| doc.timeline.clips.iter().find(|c| c.id == id));
                let path = clip.and_then(|c| c.src.as_deref()).or(args["path"].as_str()).ok_or("path or clip required")?;
                media_file(app, path)?;
                let language = args["language"].as_str().unwrap_or("auto");
                let fps = doc.project.sequence.fps;
                match speech::wait(app, &dir, path, language, WAIT).map_err(|e| format!("refused ({}): {}", e.code, e.message))? {
                    speech::Status::NoModel => Ok("needsSpeechModel: the speech model is not installed".into()),
                    speech::Status::Running(p) => Ok(format!("pending: {:.0}%, call again later", p * 100.0)),
                    speech::Status::Ready(t) => {
                        let lines: Vec<String> = speech::phrases(&t.words, 14, 90, 700)
                            .into_iter()
                            .filter_map(|p| match clip {
                                Some(c) => Some(format!("f{} {}", speech::frame_on_clip(c, p.start, fps)?, p.text)),
                                None => Some(format!("{:.2}s {}", f64::from(p.start) / 1000.0, p.text)),
                            })
                            .collect();
                        Ok(if lines.is_empty() { "no speech".into() } else { lines.join("\n") })
                    }
                }
            }
            "speech_find" => {
                let (dir, doc) = app.state::<ProjectState>().context().ok_or("no project is open")?;
                let query = args["query"].as_str().ok_or("query required")?.to_lowercase();
                let needle: Vec<&str> = query.split_whitespace().collect();
                let fps = doc.project.sequence.fps;
                let mut out = Vec::new();
                for c in doc.timeline.clips.iter().filter(|c| c.src.is_some()) {
                    let Some(t) = c.src.as_deref().and_then(|s| speech::cached_for(&dir, s)) else { continue };
                    let clean = |w: &str| w.to_lowercase().trim_matches(|x: char| !x.is_alphanumeric()).to_owned();
                    for w in t.words.windows(needle.len().max(1)) {
                        if needle.iter().zip(w).all(|(n, x)| clean(&x.text).contains(n))
                            && let Some(f) = speech::frame_on_clip(c, w[0].start, fps)
                        {
                            out.push(format!("{} f{f}", c.id));
                        }
                    }
                }
                out.dedup();
                Ok(if out.is_empty() { "not found (only transcribed clips are searched)".into() } else { out.join("\n") })
            }
            "audio_analyze" => {
                let path = args["path"].as_str().ok_or("path required")?;
                let file = media_file(app, path)?;
                let ff = ffmpeg(app)?;
                let l = audio::loudness(&ff, &file).map_err(|e| e.message)?;
                let samples = audio::decode(&ff, &file, Band::Full).map_err(|e| e.message)?;
                let pauses = audio::silences(&samples, audio::RATE, args["threshold"].as_f64().unwrap_or(-35.0), args["min_pause"].as_f64().unwrap_or(0.5));
                let shown: Vec<String> = pauses.iter().take(40).map(|(a, b)| format!("{a:.2}-{b:.2}")).collect();
                Ok(format!("loudness {:.1} LUFS, peak {:.1} dB, range {:.1} LU\nlength {:.2}s\npauses {}: {}", l.lufs, l.peak_db, l.range, samples.len() as f64 / f64::from(audio::RATE), pauses.len(), shown.join(" ")))
            }
            "render_frame" => {
                let frame = args["frame"].as_u64().and_then(|f| u32::try_from(f).ok()).ok_or("frame required")?;
                let scale = args["scale"].as_f64().unwrap_or(0.4).clamp(0.1, 1.0);
                crate::export::still(app, frame, scale).map(|p| p.display().to_string()).map_err(|e| format!("refused ({}): {}", e.code, e.message))
            }
            "export_start" => {
                let settings = crate::export::ExportSettings { name: args["name"].as_str().unwrap_or("").to_owned(), quality: args["quality"].as_str().unwrap_or("high").to_owned(), ..crate::export::ExportSettings::default() };
                crate::export::start(app, &settings).map(|p| format!("started: {}", p.display())).map_err(|e| format!("refused ({}): {}", e.code, e.message))
            }
            "export_status" => {
                let s = crate::export::status(app).map_err(|e| e.message)?;
                let list: Vec<String> = s.files.iter().take(5).map(|f| format!("{} {:.1} MB {:.1}s {:.1} Mbps", f.name, f.size_mb, f.duration_s.unwrap_or(0.0), f.mbps.unwrap_or(0.0))).collect();
                Ok(format!("engine {}, {}\n{}", if s.engine_ready { "ready" } else { "not installed" }, if s.running { "export running" } else { "idle" }, list.join("\n")))
            }
            "audio_peaks" => {
                let path = args["path"].as_str().ok_or("path required")?;
                let file = media_file(app, path)?;
                let samples = audio::decode(&ffmpeg(app)?, &file, Band::parse(args["band"].as_str().unwrap_or("bass"))).map_err(|e| e.message)?;
                let peaks = audio::peaks(&samples, audio::RATE, args["sensitivity"].as_f64().unwrap_or(50.0) / 100.0, args["min_gap"].as_f64().unwrap_or(0.25));
                let shown: Vec<String> = peaks.iter().take(300).map(|p| format!("{:.2}", p.t)).collect();
                Ok(format!("{} peaks: {}", peaks.len(), shown.join(" ")))
            }
            _ => Err(format!("unknown tool {name}")),
        }
    };
    list().as_array()?.iter().any(|t| t["name"] == name).then(run)
}
