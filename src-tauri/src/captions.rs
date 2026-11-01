//! Субтитры файлами: дорожка субтитров в режиме «субтитры видео» кладет рядом с роликом файлы для
//! загрузки на площадку; такие же файлы можно загрузить обратно на дорожку вместе с оформлением.
//! Форматы — обычный текст, разбираются вручную: чужой файл не исполняется и никуда не обращается.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::checks::{ToolId, find};
use crate::error::{AppError, AppResult};
use crate::project::ProjectState;
use crate::project::model::Snapshot;
use crate::project::timeline::{AppliedEffect, CaptionMode, Clip, ClipKind, Op, Track, TrackKind};

pub const EXTENSIONS: [&str; 3] = ["srt", "vtt", "ass"];

/// Реплика: время в секундах.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub style: Option<String>,
}

/// Оформление реплики в файле.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Look {
    pub font: Option<String>,
    pub size: Option<f64>,
    pub color: Option<String>,
    pub background: Option<String>,
    pub stroke: Option<f64>,
    pub bold: bool,
}

fn captions_track(doc: &Snapshot) -> Option<&Track> {
    doc.timeline.tracks.iter().find(|t| t.kind == TrackKind::Captions)
}

/// Дорожка субтитров в режиме файлов.
pub fn soft_track(doc: &Snapshot) -> Option<&Track> {
    captions_track(doc).filter(|t| t.caption_mode == CaptionMode::File)
}

/// Реплики дорожки. На экране верхняя заменяет нижнюю, поэтому в файле они не пересекаются.
pub fn cues_of(clips: &[Clip], track: &str, fps: u32) -> Vec<Cue> {
    let mut list: Vec<&Clip> = clips.iter().filter(|c| c.track == track && c.kind == ClipKind::Caption && c.text.as_deref().is_some_and(|t| !t.trim().is_empty())).collect();
    list.sort_by_key(|c| c.start);
    let fps = f64::from(fps.max(1));
    let mut out: Vec<Cue> = Vec::new();
    for c in list {
        let start = f64::from(c.start) / fps;
        if let Some(last) = out.last_mut() {
            last.end = last.end.min(start);
        }
        out.push(Cue { start, end: f64::from(c.end()) / fps, text: c.text.clone().unwrap_or_default(), style: None });
    }
    out.retain(|c| c.end > c.start);
    out
}

fn clock(seconds: f64, comma: bool) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}{}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, if comma { ',' } else { '.' }, ms % 1000)
}

pub fn to_srt(cues: &[Cue]) -> String {
    let mut s = String::new();
    for (i, c) in cues.iter().enumerate() {
        let _ = write!(s, "{}\n{} --> {}\n{}\n\n", i + 1, clock(c.start, true), clock(c.end, true), c.text);
    }
    s
}

pub fn to_vtt(cues: &[Cue], look: &Look) -> String {
    let mut s = String::from("WEBVTT\n\n");
    let mut css = String::new();
    if let Some(c) = &look.color {
        let _ = write!(css, "color: {c}; ");
    }
    if let Some(f) = &look.font {
        let _ = write!(css, "font-family: {f}; ");
    }
    if let Some(b) = &look.background {
        let _ = write!(css, "background-color: {b}; ");
    }
    if !css.is_empty() {
        let _ = write!(s, "STYLE\n::cue {{ {} }}\n\n", css.trim_end());
    }
    for c in cues {
        let _ = write!(s, "{} --> {}\n{}\n\n", clock(c.start, false), clock(c.end, false), c.text);
    }
    s
}

/// `#RRGGBB` → `&H00BBGGRR`.
fn ass_color(c: &str) -> Option<String> {
    let h = c.strip_prefix('#')?;
    (h.len() >= 6 && h.is_ascii()).then(|| format!("&H00{}{}{}", &h[4..6], &h[2..4], &h[0..2]).to_uppercase())
}

fn from_ass_color(c: &str) -> Option<String> {
    let h = c.trim().trim_start_matches("&H").trim_end_matches('&');
    (h.len() >= 6 && h.is_ascii()).then(|| {
        let tail = &h[h.len() - 6..];
        format!("#{}{}{}", &tail[4..6], &tail[2..4], &tail[0..2]).to_lowercase()
    })
}

fn ass_clock(seconds: f64) -> String {
    let cs = (seconds.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

pub fn to_ass(cues: &[Cue], look: &Look, width: u32, height: u32) -> String {
    let font = look.font.as_deref().unwrap_or("Arial");
    let size = look.size.unwrap_or(64.0);
    let color = look.color.as_deref().and_then(ass_color).unwrap_or_else(|| "&H00FFFFFF".into());
    let back = look.background.as_deref().and_then(ass_color);
    let border = if back.is_some() { 3 } else { 1 };
    let mut s = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {width}\nPlayResY: {height}\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, OutlineColour, BackColour, Bold, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV\nStyle: Default,{font},{size},{color},&H00000000,{},{},{border},{},0,2,40,40,{}\n\n[Events]\nFormat: Layer, Start, End, Style, Text\n",
        back.as_deref().unwrap_or("&H64000000"),
        if look.bold { -1 } else { 0 },
        look.stroke.unwrap_or(2.0),
        u32::try_from(u64::from(height) * 34 / 100).unwrap_or(0),
    );
    for c in cues {
        let _ = writeln!(s, "Dialogue: 0,{},{},Default,{}", ass_clock(c.start), ass_clock(c.end), c.text.replace('\n', "\\N"));
    }
    s
}

fn parse_clock(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    let parts: Vec<&str> = t.split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [h, m, s] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    Some(h * 3600.0 + m * 60.0 + s)
}

fn strip_tags(line: &str) -> String {
    let (mut out, mut depth) = (String::new(), 0_u32);
    for ch in line.chars() {
        match ch {
            '<' | '{' => depth += 1,
            '>' | '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// `.srt` и `.vtt`: блоки с временем через `-->`, оформление `::cue` запоминается.
pub fn parse_text_cues(raw: &str) -> (Vec<Cue>, Look) {
    let mut look = Look::default();
    let body = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in body.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        if lines.first().is_some_and(|l| l.starts_with("STYLE")) {
            for prop in block.split(['{', '}', ';']) {
                let Some((k, v)) = prop.split_once(':') else { continue };
                match k.trim() {
                    "color" => look.color = Some(v.trim().to_owned()),
                    "font-family" => look.font = Some(v.trim().to_owned()),
                    "background-color" => look.background = Some(v.trim().to_owned()),
                    _ => {}
                }
            }
            continue;
        }
        let Some(at) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let Some((a, b)) = lines[at].split_once("-->") else { continue };
        // После времени у VTT могут идти настройки положения.
        let end = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_clock(a), parse_clock(end)) else { continue };
        let text = lines[at + 1..].iter().map(|l| strip_tags(l)).collect::<Vec<_>>().join("\n");
        if end > start && !text.trim().is_empty() {
            cues.push(Cue { start, end, text, style: None });
        }
    }
    (cues, look)
}

/// `.ass`: реплики `Dialogue` и таблица стилей.
pub fn parse_ass(raw: &str) -> (Vec<Cue>, HashMap<String, Look>) {
    let (mut styles, mut cues) = (HashMap::new(), Vec::new());
    let (mut style_format, mut event_format): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut section = String::new();
    let columns = |s: &str| s.split(',').map(|x| x.trim().to_lowercase()).collect::<Vec<_>>();
    for line in raw.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.to_lowercase();
        } else if let Some(f) = line.strip_prefix("Format:") {
            if section.contains("styles") {
                style_format = columns(f);
            } else {
                event_format = columns(f);
            }
        } else if let Some(v) = line.strip_prefix("Style:") {
            let values: Vec<&str> = v.split(',').map(str::trim).collect();
            let get = |k: &str| style_format.iter().position(|x| x == k).and_then(|i| values.get(i).copied());
            let look = Look { font: get("fontname").map(str::to_owned), size: get("fontsize").and_then(|x| x.parse().ok()), color: get("primarycolour").and_then(from_ass_color), background: (get("borderstyle") == Some("3")).then(|| get("backcolour").and_then(from_ass_color)).flatten(), stroke: get("outline").and_then(|x| x.parse().ok()), bold: get("bold") == Some("-1") };
            styles.insert(get("name").unwrap_or("Default").to_owned(), look);
        } else if let Some(v) = line.strip_prefix("Dialogue:") {
            // Текст — последняя колонка, в нем могут быть запятые.
            let values: Vec<&str> = v.splitn(event_format.len().max(1), ',').map(str::trim).collect();
            let get = |k: &str| event_format.iter().position(|x| x == k).and_then(|i| values.get(i).copied());
            let (Some(start), Some(end)) = (get("start").and_then(parse_clock), get("end").and_then(parse_clock)) else { continue };
            let text = strip_tags(get("text").unwrap_or("")).replace("\\N", "\n").replace("\\n", "\n");
            if end > start && !text.trim().is_empty() {
                cues.push(Cue { start, end, text, style: get("style").map(str::to_owned) });
            }
        }
    }
    (cues, styles)
}

/// Оформление клипа субтитров: первое со `spec.text` среди его эффектов.
pub fn look_of(clip: &Clip) -> Look {
    for fx in clip.effects.iter().filter(|f| f.enabled) {
        let text = &fx.spec["text"];
        if text.is_null() {
            continue;
        }
        let read = |k: &str| match text[k].as_str() {
            Some(s) if s.starts_with('$') => fx.params[&s[1..]].clone(),
            _ => text[k].clone(),
        };
        return Look { font: read("font").as_str().map(str::to_owned), size: read("size").as_f64(), color: read("color").as_str().map(str::to_owned), background: read("background").as_str().filter(|b| !b.ends_with("00")).map(str::to_owned), stroke: read("stroke").as_f64(), bold: read("weight").as_f64().is_some_and(|w| w >= 600.0) };
    }
    Look::default()
}

/// Оформление как эффект на клипе (копия на клипе: проект не зависит от файла).
fn effect_of(look: &Look) -> Option<AppliedEffect> {
    let mut text = serde_json::Map::new();
    if let Some(f) = &look.font {
        text.insert("font".into(), json!(f));
    }
    if let Some(s) = look.size {
        text.insert("size".into(), json!(s));
    }
    if let Some(c) = &look.color {
        text.insert("color".into(), json!(c));
    }
    if let Some(b) = &look.background {
        text.insert("background".into(), json!(b));
    }
    if let Some(s) = look.stroke {
        text.insert("stroke".into(), json!(s));
    }
    if look.bold {
        text.insert("weight".into(), json!(700));
    }
    (!text.is_empty()).then(|| AppliedEffect { id: String::new(), effect: "captions-file-look".into(), name: json!({ "ru": "Оформление из файла", "en": "Look from the file" }), enabled: true, params: json!({}), controls: json!([]), spec: json!({ "tracks": [], "text": text }) })
}

/// Файлы субтитров рядом с роликом. Итог — их пути.
pub fn write_beside(doc: &Snapshot, video: &Path) -> AppResult<Vec<String>> {
    let track = captions_track(doc).ok_or_else(|| AppError::new("invalidEdit", "no captions track"))?;
    let seq = &doc.project.sequence;
    let cues = cues_of(&doc.timeline.clips, &track.id, seq.fps);
    if cues.is_empty() {
        return Err(AppError::new("noCaptions", "the captions track is empty"));
    }
    let look = doc.timeline.clips.iter().filter(|c| c.track == track.id).map(look_of).find(|l| *l != Look::default()).unwrap_or_default();
    let mut paths = Vec::new();
    for (ext, body) in [("srt", to_srt(&cues)), ("vtt", to_vtt(&cues, &look)), ("ass", to_ass(&cues, &look, seq.width, seq.height))] {
        let p = video.with_extension(ext);
        std::fs::write(&p, body)?;
        paths.push(p.display().to_string());
    }
    Ok(paths)
}

/// Сохранить файлы субтитров сейчас, без рендера: в папку готовых роликов, под именем проекта.
pub fn save_all(app: &AppHandle) -> AppResult<Vec<String>> {
    let (dir, doc) = app.state::<ProjectState>().context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
    let out = crate::export::out_dir(&dir);
    std::fs::create_dir_all(&out)?;
    write_beside(&doc, &out.join(format!("{}.mp4", crate::export::safe_name(&doc.project.name))))
}

/// Загрузить файл субтитров на дорожку вместо того, что на ней было (один шаг отмены).
pub fn import_file(app: &AppHandle, path: &str) -> AppResult<usize> {
    let file = PathBuf::from(path);
    let ext = file.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(AppError::new("badCaptions", format!("{ext}: need .srt, .vtt or .ass")));
    }
    let raw = std::fs::read(&file)?;
    let raw = String::from_utf8_lossy(&raw);
    let (cues, styles) = if ext == "ass" {
        parse_ass(&raw)
    } else {
        let (c, l) = parse_text_cues(&raw);
        (c, HashMap::from([(String::new(), l)]))
    };
    if cues.is_empty() {
        return Err(AppError::new("badCaptions", "no captions in the file"));
    }
    let state = app.state::<ProjectState>();
    let view = state.edit("captions", |doc| {
        let fps = f64::from(doc.project.sequence.fps);
        let track = captions_track(doc).map(|t| t.id.clone()).ok_or_else(|| AppError::new("invalidEdit", "no captions track"))?;
        let t = &mut doc.timeline;
        let mut e = crate::project::timeline::Edit { tracks: &mut t.tracks, clips: &mut t.clips, markers: &mut t.markers };
        let old: Vec<String> = e.clips.iter().filter(|c| c.track == track).map(|c| c.id.clone()).collect();
        if !old.is_empty() {
            e.apply(&Op::DeleteClips { ids: old, ripple: false })?;
        }
        for c in &cues {
            let start = (c.start * fps).round() as u32;
            let end = ((c.end * fps).round() as u32).max(start + 1);
            for id in e.apply(&Op::AddClip { track: track.clone(), kind: ClipKind::Caption, src: None, text: Some(c.text.clone()), start, duration: end - start, offset: None })? {
                let look = styles.get(c.style.as_deref().unwrap_or("")).or_else(|| styles.values().next());
                if let Some(fx) = look.and_then(effect_of) {
                    e.apply(&Op::AddEffect { clip: id, effect: fx })?;
                }
            }
        }
        Ok(())
    })?;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tauri_specta::Event::emit(&crate::project::ProjectChanged(Some(view)), app)));
    Ok(cues.len())
}

/// Кадр для предпросмотра на карточке: уменьшается и встраивается в описание элемента.
pub fn thumbnail_data(app: &AppHandle, file: &Path) -> AppResult<String> {
    let ffmpeg = find(app, ToolId::Ffmpeg).ok_or_else(|| AppError::new("ffmpegMissing", "ffmpeg not found"))?;
    let tmp = std::env::temp_dir().join(format!("studio-preview-{}.jpg", std::process::id()));
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-v", "error", "-y", "-i"]).arg(file).args(["-vf", "scale=320:-2", "-frames:v", "1", "-q:v", "6"]).arg(&tmp);
    let r = crate::media::quiet(&mut cmd).output().map_err(|e| AppError::new("ffmpeg", e.to_string()))?;
    if !r.status.success() {
        return Err(AppError::new("ffmpeg", String::from_utf8_lossy(&r.stderr).into_owned()));
    }
    let raw = std::fs::read(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    Ok(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(raw)))
}

#[tauri::command]
#[specta::specta]
pub fn captions_save(app: AppHandle) -> AppResult<Vec<String>> {
    save_all(&app)
}

#[tauri::command]
#[specta::specta]
pub fn captions_import(app: AppHandle, path: String) -> AppResult<u32> {
    import_file(&app, &path).map(|n| u32::try_from(n).unwrap_or(u32::MAX))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::timeline::ClipProps;

    fn cap(id: &str, start: u32, duration: u32, text: &str) -> Clip {
        Clip { id: id.into(), track: "s1".into(), kind: ClipKind::Caption, src: None, text: Some(text.into()), start, duration, offset: 0, link: None, props: ClipProps::default(), effects: vec![] }
    }

    #[test]
    fn overlapping_captions_do_not_overlap_in_the_file() {
        let cues = cues_of(&[cap("a", 0, 60, "первая"), cap("b", 30, 30, "вторая"), cap("c", 90, 0, "пусто")], "s1", 30);
        assert_eq!(cues.len(), 2);
        assert!((cues[0].end - 1.0).abs() < 1e-9, "нижнюю заменила верхняя");
        assert!((cues[1].start - 1.0).abs() < 1e-9);
    }

    #[test]
    fn srt_and_vtt_survive_a_round_trip() {
        let cues = vec![Cue { start: 1.5, end: 3.25, text: "Привет, мир".into(), style: None }, Cue { start: 3661.0, end: 3662.0, text: "две\nстроки".into(), style: None }];
        let srt = to_srt(&cues);
        assert!(srt.contains("00:00:01,500 --> 00:00:03,250"));
        assert_eq!(parse_text_cues(&srt).0, cues);
        let look = Look { color: Some("#ffcc00".into()), font: Some("Arial".into()), ..Look::default() };
        let (back, read) = parse_text_cues(&to_vtt(&cues, &look));
        assert_eq!(back, cues);
        assert_eq!((read.color.as_deref(), read.font.as_deref()), (Some("#ffcc00"), Some("Arial")));
    }

    #[test]
    fn foreign_files_are_read_without_failing() {
        let vtt = "\u{feff}WEBVTT\n\nNOTE заметка\n\n00:01.000 --> 00:02.000 line:90%\n<b>жирный</b> текст\n\nмусор без времени\n\n00:03.000 --> 00:02.000\nназад\n";
        let (cues, _) = parse_text_cues(vtt);
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "жирный текст");
    }

    #[test]
    fn ass_keeps_the_look_and_commas_in_text() {
        let cues = vec![Cue { start: 0.5, end: 2.0, text: "раз, два\nтри".into(), style: None }];
        let look = Look { font: Some("Golos".into()), size: Some(72.0), color: Some("#ff0000".into()), background: Some("#000000".into()), stroke: Some(3.0), bold: true };
        let ass = to_ass(&cues, &look, 1080, 1920);
        assert!(ass.contains("Style: Default,Golos,72,&H000000FF"));
        let (back, styles) = parse_ass(&ass);
        assert_eq!(back[0].text, "раз, два\nтри");
        assert_eq!((back[0].start, back[0].end), (0.5, 2.0));
        assert_eq!(styles["Default"], look);
    }

    #[test]
    fn preview_look_comes_from_the_clip_effect() {
        let mut c = cap("a", 0, 10, "x");
        c.effects.push(AppliedEffect { id: "fx1".into(), effect: "t".into(), name: json!("t"), enabled: true, params: json!({ "c": "#112233" }), controls: json!([]), spec: json!({ "text": { "color": "$c", "size": 80, "weight": 700 } }) });
        let l = look_of(&c);
        assert_eq!((l.color.as_deref(), l.size, l.bold), (Some("#112233"), Some(80.0), true));
    }
}
