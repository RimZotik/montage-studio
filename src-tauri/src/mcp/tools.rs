//! Инструменты MCP студии. Описание каждого — коротко (оно уходит в контекст Claude каждый
//! ход), ответы — строками текста вместо JSON. Правки — через сервис проекта (проверка, история,
//! событие в интерфейс), подпись хода `claude#N` — отмена хода одним действием.

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use super::{McpState, UiCommand};
use crate::project::timeline::{Clip, Op};
use crate::project::{ProjectChanged, ProjectState};

/// Событие интерфейсу; отсутствие подписки (тестовое приложение) не роняет сервер.
fn send<R: tauri::Runtime, E: Event + serde::Serialize + Clone>(app: &AppHandle<R>, event: E) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| event.emit(app)));
}

pub fn list() -> Value {
    json!([
        { "name": "project_summary", "description": "Project, sequence, tracks with clip counts, total length, markers, playhead, selection, open tab.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "media_list", "description": "Project media files: path, category, duration, size. Optional category filter: footage|voice|music|sfx|images.", "inputSchema": { "type": "object", "properties": { "category": { "type": "string" } } } },
        { "name": "timeline_get", "description": "Clips overlapping [from,to) frames (default all), optional track id. One line per clip.", "inputSchema": { "type": "object", "properties": { "from": { "type": "integer" }, "to": { "type": "integer" }, "track": { "type": "string" } } } },
        { "name": "timeline_apply", "description": "Apply a batch of timeline ops atomically (one undo step). Times in frames. Ops: {op:addClip,track,kind:media|caption|solid,src?,text?,start,duration,offset?} {op:addLinked,videoTrack,audioTrack,src,start,duration,offset?} (video with its sound, moved/split/deleted together) {op:moveClip,id,track?,start} {op:trimClip,id,start?,duration?,offset?} {op:splitClip,id,at} {op:deleteClips,ids,ripple} {op:setProps,id,props:{volume,opacity,scale,x,y,fadeIn,fadeOut,speed}} (only the given fields change) {op:setText,id,text} {op:setTrack,id,muted?,locked?,volume?} {op:setEffect,clip,fx,params?,enabled?,controls?,spec?} {op:removeEffect,clip,fx} {op:addMarker,frame,label} {op:removeMarker,frame}. Clips may overlap on one track: the later one is on top. Caption position: props.y (px from default lower third, negative = up). Solid clip: text is a CSS color or gradient. Returns created clip ids or the reason it was refused.", "inputSchema": { "type": "object", "properties": { "ops": { "type": "array", "items": { "type": "object" } }, "label": { "type": "string" } }, "required": ["ops"] } },
        { "name": "ui_seek", "description": "Move the playhead to a frame (or seconds).", "inputSchema": { "type": "object", "properties": { "frame": { "type": "integer" }, "seconds": { "type": "number" } } } },
        { "name": "ui_select", "description": "Select clips on the timeline by ids (empty — clear).", "inputSchema": { "type": "object", "properties": { "ids": { "type": "array", "items": { "type": "string" } } }, "required": ["ids"] } },
        { "name": "ui_tab", "description": "Open a project tab: prepare|edit|export.", "inputSchema": { "type": "object", "properties": { "tab": { "type": "string" } }, "required": ["tab"] } },
        { "name": "log_errors", "description": "Last warnings and errors from the studio log (only when the user asks to look into a problem).", "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer" } } } },
    ])
}

fn project<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<(std::path::PathBuf, crate::project::model::Snapshot), String> {
    app.state::<ProjectState>().context().ok_or_else(|| "no project is open".to_owned())
}

fn clip_line(c: &Clip) -> String {
    let what = match (&c.src, &c.text) {
        (Some(s), _) => s.clone(),
        (None, Some(t)) => format!("\"{t}\""),
        _ => String::new(),
    };
    let p = &c.props;
    let mut extra = Vec::new();
    if (p.volume - 1.0).abs() > 1e-9 {
        extra.push(format!("vol={}", p.volume));
    }
    if (p.opacity - 1.0).abs() > 1e-9 {
        extra.push(format!("op={}", p.opacity));
    }
    if (p.scale - 1.0).abs() > 1e-9 || p.x != 0.0 || p.y != 0.0 {
        extra.push(format!("scale={} x={} y={}", p.scale, p.x, p.y));
    }
    if p.fade_in > 0 || p.fade_out > 0 {
        extra.push(format!("fade={}/{}", p.fade_in, p.fade_out));
    }
    if (p.speed - 1.0).abs() > 1e-9 {
        extra.push(format!("speed={}", p.speed));
    }
    if !p.volume_keys.is_empty() {
        extra.push(format!("volkeys={}", p.volume_keys.len()));
    }
    for fx in &c.effects {
        extra.push(format!("{}={}{}{}", fx.id, fx.effect, fx.params, if fx.enabled { "" } else { " off" }));
    }
    format!("{} {} {:?} {} start={} dur={} off={} {}", c.id, c.track, c.kind, what, c.start, c.duration, c.offset, extra.join(" ")).to_lowercase().trim_end().to_owned()
}

pub fn call<R: tauri::Runtime>(app: &AppHandle<R>, name: &str, args: &Value) -> Result<String, String> {
    let mcp = app.state::<McpState>();
    match name {
        "project_summary" => {
            let (dir, doc) = project(app)?;
            let seq = &doc.project.sequence;
            let t = &doc.timeline;
            let total = t.clips.iter().map(Clip::end).max().unwrap_or(0);
            let ui = mcp.ui.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            let tracks: Vec<String> = t.tracks.iter().map(|tr| format!("{} {:?}{}{} ({} clips)", tr.id, tr.kind, if tr.muted { " muted" } else { "" }, if tr.locked { " locked" } else { "" }, t.clips.iter().filter(|c| c.track == tr.id).count()).to_lowercase()).collect();
            let markers: Vec<String> = t.markers.iter().map(|m| format!("{}{}", m.frame, if m.label.is_empty() { String::new() } else { format!(" {}", m.label) })).collect();
            Ok(format!(
                "project: {} ({})\nsequence: {}x{} {}fps background {}\nlength: {} frames ({:.2}s)\ntracks (top first): {}\nmarkers: {}\nplayhead: {}\nselected: {}\ntab: {}",
                doc.project.name,
                dir.display(),
                seq.width,
                seq.height,
                seq.fps,
                seq.background,
                total,
                f64::from(total) / f64::from(seq.fps.max(1)),
                tracks.join("; "),
                if markers.is_empty() { "-".into() } else { markers.join(", ") },
                ui.frame,
                if ui.selected.is_empty() { "-".into() } else { ui.selected.join(",") },
                if ui.tab.is_empty() { "-" } else { &ui.tab },
            ))
        }
        "media_list" => {
            let (dir, _) = project(app)?;
            let want = args["category"].as_str();
            let items = crate::media::list(&dir, None, None).map_err(|e| e.message)?;
            let lines: Vec<String> = items
                .iter()
                .filter(|i| want.is_none_or(|w| w == i.category))
                .map(|i| {
                    let p = i.probe.as_ref();
                    let dur = p.and_then(|p| p.duration_s).map_or(String::new(), |d| format!(" {d:.2}s"));
                    let size = p.and_then(|p| p.width.zip(p.height)).map_or(String::new(), |(w, h)| format!(" {w}x{h}"));
                    format!("{} [{}]{dur}{size}{}", i.path, i.category, if p.is_some_and(|p| p.audio) { " audio" } else { "" })
                })
                .collect();
            Ok(if lines.is_empty() { "no media".into() } else { lines.join("\n") })
        }
        "timeline_get" => {
            let (_, doc) = project(app)?;
            let from = args["from"].as_u64().unwrap_or(0);
            let to = args["to"].as_u64().unwrap_or(u64::MAX);
            let track = args["track"].as_str();
            let mut clips: Vec<&Clip> = doc.timeline.clips.iter().filter(|c| u64::from(c.end()) > from && u64::from(c.start) < to && track.is_none_or(|t| t == c.track)).collect();
            clips.sort_by_key(|c| (c.track.clone(), c.start));
            Ok(if clips.is_empty() { "no clips".into() } else { clips.iter().map(|c| clip_line(c)).collect::<Vec<_>>().join("\n") })
        }
        "timeline_apply" => {
            let ops: Vec<Op> = serde_json::from_value(args["ops"].clone()).map_err(|e| format!("bad ops: {e}"))?;
            let turn = mcp.turn.load(std::sync::atomic::Ordering::SeqCst);
            send(app, UiCommand::Busy { area: "timeline".into() });
            let mut created = Vec::new();
            let state = app.state::<ProjectState>();
            let view = state
                .edit(&format!("claude#{turn}"), |doc| {
                    let t = &mut doc.timeline;
                    let mut e = crate::project::timeline::Edit { tracks: &mut t.tracks, clips: &mut t.clips, markers: &mut t.markers };
                    for op in &ops {
                        created.extend(e.apply(op)?);
                    }
                    Ok(())
                })
                .map_err(|e| format!("refused ({}): {}", e.code, e.message))?;
            send(app, ProjectChanged(Some(view)));
            Ok(if created.is_empty() { "ok".into() } else { format!("ok, created: {}", created.join(",")) })
        }
        "ui_seek" => {
            let (_, doc) = project(app)?;
            let fps = f64::from(doc.project.sequence.fps);
            let frame = args["frame"].as_u64().map(|f| f as f64).or_else(|| args["seconds"].as_f64().map(|s| s * fps)).ok_or("frame or seconds required")?;
            send(app, UiCommand::Seek { frame: frame.max(0.0).round() as u32 });
            Ok("ok".into())
        }
        "ui_select" => {
            let ids: Vec<String> = serde_json::from_value(args["ids"].clone()).map_err(|e| e.to_string())?;
            send(app, UiCommand::Select { ids });
            Ok("ok".into())
        }
        "ui_tab" => {
            let tab = args["tab"].as_str().filter(|t| ["prepare", "edit", "export"].contains(t)).ok_or("tab: prepare|edit|export")?;
            send(app, UiCommand::Tab { tab: tab.into() });
            Ok("ok".into())
        }
        "log_errors" => {
            let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
            let text = std::fs::read_to_string(dir.join("errors.log")).unwrap_or_default();
            let limit = args["limit"].as_u64().unwrap_or(40).min(200) as usize;
            let lines: Vec<&str> = text.lines().collect();
            let tail = &lines[lines.len().saturating_sub(limit)..];
            Ok(if tail.is_empty() { "no errors logged".into() } else { tail.join("\n") })
        }
        _ => Err(format!("unknown tool {name}")),
    }
}
