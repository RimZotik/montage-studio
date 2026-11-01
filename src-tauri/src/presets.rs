//! Запуск инструментов библиотеки (`docs/extensions.md`): встроенные алгоритмы (нарезка под бит,
//! вырезание пауз, субтитры по голосу, нормализация, приглушение музыки) и шаблоны операций.
//! Расчет — чистые функции, итог — операции таймлайна одним шагом отмены через сервис проекта.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::audio::{self, Band};
use crate::checks::{ToolId, find};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::library::{self, Item};
use crate::project::model::Snapshot;
use crate::project::timeline::{AppliedEffect, Clip, ClipKind, ClipPropsPatch, Edit, Op, TrackKind};
use crate::project::{ProjectChanged, ProjectState};
use crate::speech;

/// Свободный JSON от интерфейса (значения панели).
#[derive(Debug, Clone, serde::Deserialize, Type)]
#[serde(transparent)]
pub struct Json(#[specta(type = specta_typescript::Any)] pub Value);

/// Итог запуска.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutcome {
    /// `done`, `pending` (расшифровка еще идет, запустить снова) или `needsSpeechModel`.
    pub status: String,
    /// Сколько клипов создано или изменено.
    pub changed: u32,
    /// 0–1 у `pending`.
    pub progress: f64,
    /// Короткий итог (для Claude и подсказки).
    pub note: String,
}

fn done(changed: usize, note: String) -> ToolOutcome {
    ToolOutcome { status: "done".into(), changed: u32::try_from(changed).unwrap_or(0), progress: 1.0, note }
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::new("toolFailed", msg)
}

fn num(p: &Value, key: &str, default: f64) -> f64 {
    p[key].as_f64().unwrap_or(default)
}

fn secs_to_frames(s: f64, fps: u32) -> u32 {
    (s.max(0.0) * f64::from(fps)).round() as u32
}

/// Отрезки между ударами: `peaks` — кадры таймлайна по возрастанию, берется каждый `every`-й;
/// первый отрезок — от начала. Короче `min` кадров не бывает.
pub fn beat_segments(peaks: &[u32], start: u32, end: u32, every: usize, min: u32) -> Vec<(u32, u32)> {
    let mut cuts: Vec<u32> = vec![start];
    cuts.extend(peaks.iter().copied().filter(|p| *p > start && *p < end).step_by(every.max(1)));
    cuts.push(end);
    let mut out: Vec<(u32, u32)> = Vec::new();
    for w in cuts.windows(2) {
        let (a, b) = (w[0], w[1]);
        match out.last_mut() {
            // Слишком короткий кусок добавляется к прошлому: кадры не теряются.
            Some(last) if b - a < min.max(1) => last.1 += b - a,
            _ => out.push((a, b - a)),
        }
    }
    out
}

/// Источник нарезки.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub src: String,
    pub offset: u32,
    /// Длина файла в кадрах; `None` — картинка.
    pub frames: Option<u32>,
}

/// Псевдослучайный порядок с постоянным зерном: один и тот же запуск дает один и тот же монтаж.
fn next_rand(state: &mut u64) -> usize {
    *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    (*state >> 33) as usize
}

/// Клипы нарезки: источники по кругу или вперемешку; у видео каждый следующий кусок берется дальше
/// по файлу, в конце файла — снова с начала.
pub fn beat_clips(segments: &[(u32, u32)], sources: &[Source], shuffle: bool, track: &str) -> Vec<Op> {
    let mut cursor: Vec<u32> = sources.iter().map(|s| s.offset).collect();
    let mut seed = 42u64;
    let mut last = usize::MAX;
    let mut ops = Vec::new();
    for (n, (start, duration)) in segments.iter().enumerate() {
        let mut i = if shuffle { next_rand(&mut seed) % sources.len() } else { n % sources.len() };
        if shuffle && i == last && sources.len() > 1 {
            i = (i + 1) % sources.len();
        }
        last = i;
        let s = &sources[i];
        let offset = match s.frames {
            None => 0,
            Some(total) => {
                if cursor[i] + duration > total {
                    cursor[i] = s.offset.min(total.saturating_sub(*duration));
                }
                let at = cursor[i];
                cursor[i] += duration;
                at
            }
        };
        ops.push(Op::AddClip { track: track.to_owned(), kind: ClipKind::Media, src: Some(s.src.clone()), text: None, start: *start, duration: *duration, offset: Some(offset) });
    }
    ops
}

/// Паузы клипа в кадрах таймлайна: края оставлены с запасом `pad`, короткие (после запаса) и
/// обрезанные участки отброшены. По возрастанию.
pub fn silence_cuts(clip: &Clip, silent_s: &[(f64, f64)], fps: u32, pad_s: f64, min_frames: u32) -> Vec<(u32, u32)> {
    let pad = secs_to_frames(pad_s, fps);
    let (begin, end) = (clip.offset, clip.offset + clip.duration);
    silent_s
        .iter()
        .filter_map(|(a, b)| {
            let a = secs_to_frames(*a, fps);
            let b = secs_to_frames(*b, fps);
            // Пауза на самом краю клипа убирается вплотную: запас нужен только рядом с речью.
            let from = if a <= begin { begin } else { a + pad };
            let to = if b >= end { end } else { b.saturating_sub(pad) };
            let from = from.max(begin);
            let to = to.min(end);
            (to > from && to - from >= min_frames.max(1)).then(|| (clip.start + from - begin, clip.start + to - begin))
        })
        .collect()
}

/// Вырезать участки из клипа со сдвигом: с конца, чтобы ранние кадры оставались верными. Субтитры
/// правее выреза едут вместе с речью.
pub fn apply_cuts(e: &mut Edit<'_>, clip_id: &str, cuts: &[(u32, u32)]) -> AppResult<usize> {
    let captions: Vec<String> = e.tracks.iter().filter(|t| t.kind == TrackKind::Captions).map(|t| t.id.clone()).collect();
    for &(a, b) in cuts.iter().rev() {
        let c = e.clips.iter().find(|c| c.id == clip_id).ok_or_else(|| bad("clip disappeared"))?.clone();
        let mut target = clip_id.to_owned();
        if b < c.end() {
            e.apply(&Op::SplitClip { id: clip_id.to_owned(), at: b })?;
        }
        if a > c.start {
            let right = e.apply(&Op::SplitClip { id: clip_id.to_owned(), at: a })?;
            target = right.into_iter().next().ok_or_else(|| bad("split gave nothing"))?;
        }
        let moved: Vec<(String, u32)> = e.clips.iter().filter(|x| captions.contains(&x.track) && x.start >= b).map(|x| (x.id.clone(), x.start - (b - a))).collect();
        let whole = a <= c.start && b >= c.end();
        if whole {
            return Err(bad("the whole clip is silent"));
        }
        e.apply(&Op::DeleteClips { ids: vec![target], ripple: true })?;
        for (id, start) in moved {
            if let Some(x) = e.clips.iter_mut().find(|x| x.id == id) {
                x.start = start;
            }
        }
    }
    Ok(cuts.len())
}

/// Ключи громкости музыки под речью. `speech` — участки речи в кадрах таймлайна; итог — в кадрах
/// файла (разрез и перенос клипа их не сбивают).
pub fn duck_keys(speech: &[(u32, u32)], clip: &Clip, gain: f64, attack: u32, release: u32) -> Vec<(u32, f64)> {
    let mut merged: Vec<(u32, u32)> = Vec::new();
    let mut sorted = speech.to_vec();
    sorted.sort_unstable();
    for (a, b) in sorted {
        match merged.last_mut() {
            // Короткая пауза: музыка не успевает подняться — не дергать.
            Some(last) if a <= last.1 + attack + release => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let local = |f: i64| u32::try_from((f - i64::from(clip.start) + i64::from(clip.offset)).max(0)).unwrap_or(0);
    let mut keys: Vec<(u32, f64)> = Vec::new();
    for (a, b) in merged {
        if b <= clip.start || a >= clip.end() {
            continue;
        }
        let (a, b) = (i64::from(a), i64::from(b));
        let down = local(a - i64::from(attack));
        if keys.last().is_none_or(|k| k.0 < down) {
            keys.push((down, 1.0));
        }
        keys.push((local(a).max(down + 1), gain));
        keys.push((local(b).max(down + 2), gain));
        keys.push((local(b + i64::from(release)).max(down + 3), 1.0));
    }
    keys
}

/// Простое выражение панели: `$a`, `$clip.start+30`, `$seconds*$fps` (слева направо, без скобок).
pub fn eval(expr: &str, vars: &HashMap<String, Value>) -> Option<Value> {
    let body = expr.trim();
    if !body.contains('$') {
        return None;
    }
    // Ровно одно имя: значение как есть (может быть строкой, например ID дорожки).
    if let Some(name) = body.strip_prefix('$')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return vars.get(name).cloned();
    }
    let mut total: Option<f64> = None;
    let mut op = '+';
    let mut word = String::new();
    let mut apply = |word: &str, op: char| -> Option<()> {
        let w = word.trim();
        let v = match w.strip_prefix('$') {
            Some(name) => vars.get(name)?.as_f64()?,
            None => w.parse::<f64>().ok()?,
        };
        let t = total.unwrap_or(0.0);
        total = Some(match op {
            '+' => t + v,
            '-' => t - v,
            '*' => t * v,
            '/' => t / v,
            _ => return None,
        });
        Some(())
    };
    for ch in body.chars() {
        if "+-*/".contains(ch) && !word.trim().is_empty() {
            apply(&word, op)?;
            op = ch;
            word.clear();
        } else {
            word.push(ch);
        }
    }
    apply(&word, op)?;
    total.map(|t| json!(t))
}

/// Подставить значения в шаблон операции; времена — целые кадры.
pub fn fill(template: &Value, vars: &HashMap<String, Value>, key: &str) -> Value {
    match template {
        Value::String(s) if s.contains('$') => eval(s, vars).map_or(Value::Null, |v| match (key, v.as_f64()) {
            ("start" | "duration" | "offset" | "at" | "frame", Some(n)) => json!(n.max(0.0).round() as u64),
            _ => v,
        }),
        Value::Array(a) => Value::Array(a.iter().map(|x| fill(x, vars, key)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), fill(v, vars, k))).collect()),
        other => other.clone(),
    }
}

/// Выбранные клипы, или все подходящие на дорожке `track`.
fn targets<'a>(doc: &'a Snapshot, ids: &[String], track: Option<&str>, accept: impl Fn(&Clip) -> bool) -> Vec<&'a Clip> {
    let picked: Vec<&Clip> = doc.timeline.clips.iter().filter(|c| ids.contains(&c.id) && accept(c)).collect();
    if !picked.is_empty() {
        return picked;
    }
    doc.timeline.clips.iter().filter(|c| track.is_some_and(|t| c.track == t) && accept(c)).collect()
}

fn track_kind(doc: &Snapshot, track: &str) -> Option<TrackKind> {
    doc.timeline.tracks.iter().find(|t| t.id == track).map(|t| t.kind)
}

fn is_image(src: &str) -> bool {
    crate::media::category_of(src) == Some("images")
}

/// Применить правки как один шаг истории.
fn commit(app: &AppHandle, label: &str, change: impl FnOnce(&mut Edit<'_>) -> AppResult<()>) -> AppResult<()> {
    let state = app.state::<ProjectState>();
    let view = state.edit(label, |doc| {
        let t = &mut doc.timeline;
        let mut e = Edit { tracks: &mut t.tracks, clips: &mut t.clips, markers: &mut t.markers };
        change(&mut e)
    })?;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tauri_specta::Event::emit(&ProjectChanged(Some(view)), app)));
    Ok(())
}

struct Ctx<'a> {
    app: &'a AppHandle,
    dir: &'a Path,
    doc: &'a Snapshot,
    p: &'a Value,
    ids: &'a [String],
    label: &'a str,
    fps: u32,
    ffmpeg: std::path::PathBuf,
}

impl Ctx<'_> {
    fn file(&self, src: &str) -> std::path::PathBuf {
        self.dir.join(src)
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.p[key].as_str().filter(|s| !s.is_empty())
    }
}

fn effect_copy(app: &AppHandle, id: &str) -> Option<AppliedEffect> {
    let (item, _) = library::find(&library::root_of(app).ok()?, id)?;
    (item.kind == "effect").then(|| library::applied(&item, &Value::Null))
}

fn beat_cut(c: &Ctx<'_>) -> AppResult<ToolOutcome> {
    let is_visual = |x: &Clip| x.kind == ClipKind::Media && track_kind(c.doc, &x.track) == Some(TrackKind::Video);
    let first_video = c.doc.timeline.tracks.iter().rev().find(|t| t.kind == TrackKind::Video && c.doc.timeline.clips.iter().any(|x| x.track == t.id));
    let mut visuals = targets(c.doc, c.ids, c.text("sourceTrack").or(first_video.map(|t| t.id.as_str())), is_visual);
    visuals.sort_by_key(|x| x.start);
    if visuals.is_empty() {
        return Err(bad("no footage or images: select clips on a video track"));
    }
    let is_sound = |x: &Clip| x.kind == ClipKind::Media && track_kind(c.doc, &x.track) == Some(TrackKind::Audio);
    // Музыка — выбранный звук, иначе самый длинный клип на дорожке музыки; связанный звук самих
    // кусков — не музыка.
    let picked: Vec<&Clip> = targets(c.doc, c.ids, c.text("musicTrack").or(Some("a2")), is_sound).into_iter().filter(|x| !(x.link.is_some() && visuals.iter().any(|v| v.link == x.link))).collect();
    let music = picked.into_iter().max_by_key(|x| x.duration).ok_or_else(|| bad("no music: put a music clip on the music track"))?;
    let src = music.src.as_deref().ok_or_else(|| bad("music has no file"))?;
    let samples = audio::decode(&c.ffmpeg, &c.file(src), Band::parse(c.text("band").unwrap_or("bass")))?;
    let peaks = audio::peaks(&samples, audio::RATE, num(c.p, "sensitivity", 50.0) / 100.0, num(c.p, "minGap", 0.25));
    let frames: Vec<u32> = peaks.iter().filter_map(|x| speech::frame_on_clip(music, (x.t * 1000.0) as u32, c.fps)).collect();
    let limit = num(c.p, "length", 0.0);
    let end = if limit > 0.0 { music.end().min(music.start + secs_to_frames(limit, c.fps)) } else { music.end() };
    let segments = beat_segments(&frames, music.start, end, num(c.p, "every", 1.0).max(1.0) as usize, secs_to_frames(0.08, c.fps).max(2));
    let ffprobe = find(c.app, ToolId::Ffprobe);
    let mut sources: Vec<Source> = Vec::new();
    for v in &visuals {
        let Some(path) = v.src.as_deref() else { continue };
        if sources.iter().any(|s| s.src == path && s.offset == v.offset) {
            continue;
        }
        let frames = if is_image(path) { None } else { ffprobe.as_ref().and_then(|p| crate::media::probe(p, &c.file(path))).and_then(|p| p.duration_s).map(|d| secs_to_frames(d, c.fps)) };
        sources.push(Source { src: path.to_owned(), offset: v.offset, frames });
    }
    let track = visuals[0].track.clone();
    let ids: Vec<String> = visuals.iter().map(|v| v.id.clone()).collect();
    let adds = beat_clips(&segments, &sources, c.p["shuffle"].as_bool().unwrap_or(false), &track);
    let hit = c.text("hit").filter(|h| *h != "none").and_then(|h| effect_copy(c.app, h));
    let count = adds.len();
    commit(c.app, c.label, |e| {
        e.apply(&Op::DeleteClips { ids, ripple: false })?;
        for op in &adds {
            for id in e.apply(op)? {
                if let Some(fx) = &hit {
                    e.apply(&Op::AddEffect { clip: id, effect: fx.clone() })?;
                }
            }
        }
        Ok(())
    })?;
    Ok(done(count, format!("{} peaks found, {count} clips cut to music on track {track}", peaks.len())))
}

fn remove_silence(c: &Ctx<'_>) -> AppResult<ToolOutcome> {
    let audible = |x: &Clip| x.kind == ClipKind::Media && x.src.as_deref().is_some_and(|s| !is_image(s));
    let mut clips = targets(c.doc, c.ids, c.text("track").or(Some("a1")), audible);
    // Из связки — один (второй режется вместе с ним).
    let mut seen = Vec::new();
    clips.retain(|x| match &x.link {
        Some(l) if seen.contains(l) => false,
        Some(l) => {
            seen.push(l.clone());
            true
        }
        None => true,
    });
    if clips.is_empty() {
        return Err(bad("no clips with sound: select a clip or set the track"));
    }
    clips.sort_by_key(|x| std::cmp::Reverse(x.start));
    let (threshold, min_s, pad) = (num(c.p, "threshold", -35.0), num(c.p, "minPause", 0.5), num(c.p, "pad", 0.1));
    let mut plan: Vec<(String, Vec<(u32, u32)>)> = Vec::new();
    for x in &clips {
        let Some(src) = x.src.as_deref() else { continue };
        let samples = audio::decode(&c.ffmpeg, &c.file(src), Band::Full)?;
        let silent = audio::silences(&samples, audio::RATE, threshold, min_s);
        let cuts = silence_cuts(x, &silent, c.fps, pad, secs_to_frames(0.05, c.fps));
        plan.push((x.id.clone(), cuts));
    }
    let frames: u32 = plan.iter().flat_map(|(_, cuts)| cuts.iter().map(|(a, b)| b - a)).sum();
    let count: usize = plan.iter().map(|(_, cuts)| cuts.len()).sum();
    let only_mark = c.p["markersOnly"].as_bool().unwrap_or(false);
    commit(c.app, c.label, |e| {
        for (id, cuts) in &plan {
            if only_mark {
                for (a, _) in cuts {
                    e.apply(&Op::AddMarker { frame: *a, label: "pause".into() })?;
                }
            } else {
                apply_cuts(e, id, cuts)?;
            }
        }
        Ok(())
    })?;
    Ok(done(count, format!("{count} pauses, {:.1} s {}", f64::from(frames) / f64::from(c.fps), if only_mark { "marked" } else { "removed" })))
}

fn captions(c: &Ctx<'_>, wait: Duration) -> AppResult<ToolOutcome> {
    let audible = |x: &Clip| x.kind == ClipKind::Media && x.src.as_deref().is_some_and(|s| !is_image(s));
    let clips = targets(c.doc, c.ids, c.text("voiceTrack").or(Some("a1")), audible);
    if clips.is_empty() {
        return Err(bad("no voice: select a clip or put voice on the voice track"));
    }
    let language = c.text("language").unwrap_or("auto");
    let track = c.text("track").map_or_else(|| c.doc.timeline.tracks.iter().find(|t| t.kind == TrackKind::Captions).map(|t| t.id.clone()), |t| Some(t.to_owned())).ok_or_else(|| bad("no captions track"))?;
    let (words, chars) = (num(c.p, "maxWords", 4.0) as usize, num(c.p, "maxChars", 28.0) as usize);
    let mut adds = Vec::new();
    let mut seen = Vec::new();
    for x in clips {
        // Из связки — только звук одного клипа (картинка и звук говорят одно и то же).
        if let Some(l) = &x.link {
            if seen.contains(l) {
                continue;
            }
            seen.push(l.clone());
        }
        let Some(src) = x.src.as_deref() else { continue };
        match speech::wait(c.app, c.dir, src, language, wait)? {
            speech::Status::NoModel => return Ok(ToolOutcome { status: "needsSpeechModel".into(), changed: 0, progress: 0.0, note: "speech model is not installed".into() }),
            speech::Status::Running(p) => return Ok(ToolOutcome { status: "pending".into(), changed: 0, progress: p, note: format!("transcribing {src}: {:.0}%, run again in a minute", p * 100.0) }),
            speech::Status::Ready(t) => adds.extend(speech::caption_ops(x, &t.words, c.fps, words, chars, &track)),
        }
    }
    let count = adds.len();
    let replace = c.p["replace"].as_bool().unwrap_or(true);
    commit(c.app, c.label, |e| {
        if replace {
            let old: Vec<String> = e.clips.iter().filter(|x| x.track == track).map(|x| x.id.clone()).collect();
            if !old.is_empty() {
                e.apply(&Op::DeleteClips { ids: old, ripple: false })?;
            }
        }
        for op in &adds {
            e.apply(op)?;
        }
        Ok(())
    })?;
    Ok(done(count, format!("{count} captions on {track}")))
}

fn normalize(c: &Ctx<'_>) -> AppResult<ToolOutcome> {
    let audible = |x: &Clip| x.kind == ClipKind::Media && x.src.as_deref().is_some_and(|s| !is_image(s));
    let clips = targets(c.doc, c.ids, c.text("track").or(Some("a1")), audible);
    if clips.is_empty() {
        return Err(bad("no clips with sound"));
    }
    let target = num(c.p, "target", -14.0);
    let mut measured: HashMap<&str, audio::Loudness> = HashMap::new();
    let mut ops = Vec::new();
    let mut clipped = 0;
    for x in &clips {
        let Some(src) = x.src.as_deref() else { continue };
        if !measured.contains_key(src) {
            measured.insert(src, audio::loudness(&c.ffmpeg, &c.file(src))?);
        }
        let gain = audio::gain_to(measured[src].lufs, target);
        if gain > 2.0 {
            clipped += 1;
        }
        ops.push(Op::SetProps { id: x.id.clone(), props: ClipPropsPatch { volume: Some(gain.min(2.0)), ..ClipPropsPatch::default() } });
    }
    let count = ops.len();
    commit(c.app, c.label, |e| {
        for op in &ops {
            e.apply(op)?;
        }
        Ok(())
    })?;
    let report: Vec<String> = measured.iter().map(|(k, v)| format!("{}: {:.1} LUFS", k.rsplit('/').next().unwrap_or(k), v.lufs)).collect();
    Ok(done(count, format!("target {target} LUFS; {}{}", report.join(", "), if clipped > 0 { "; too quiet to reach the target, set to the maximum" } else { "" })))
}

fn duck(c: &Ctx<'_>) -> AppResult<ToolOutcome> {
    let voice_track = c.text("voiceTrack").unwrap_or("a1");
    let is_sound = |x: &Clip| x.kind == ClipKind::Media && track_kind(c.doc, &x.track) == Some(TrackKind::Audio);
    let voices: Vec<&Clip> = c.doc.timeline.clips.iter().filter(|x| x.track == voice_track && is_sound(x)).collect();
    if voices.is_empty() {
        return Err(bad(format!("no voice on track {voice_track}")));
    }
    let music: Vec<&Clip> = targets(c.doc, c.ids, c.text("musicTrack").or(Some("a2")), is_sound).into_iter().filter(|x| x.track != voice_track).collect();
    if music.is_empty() {
        return Err(bad("no music: select clips or set the music track"));
    }
    let mut decoded: HashMap<&str, Vec<(f64, f64)>> = HashMap::new();
    let mut speech_frames: Vec<(u32, u32)> = Vec::new();
    for v in &voices {
        let Some(src) = v.src.as_deref() else { continue };
        if !decoded.contains_key(src) {
            let s = audio::decode(&c.ffmpeg, &c.file(src), Band::Full)?;
            let total = s.len() as f64 / f64::from(audio::RATE);
            decoded.insert(src, audio::sounding(&audio::silences(&s, audio::RATE, num(c.p, "threshold", -35.0), 0.25), total));
        }
        for (a, b) in &decoded[src] {
            let (a, b) = (secs_to_frames(*a, c.fps), secs_to_frames(*b, c.fps));
            let (from, to) = (a.max(v.offset), b.min(v.offset + v.duration));
            if to > from {
                speech_frames.push((v.start + from - v.offset, v.start + to - v.offset));
            }
        }
    }
    let gain = 10f64.powf(num(c.p, "amount", -12.0).min(0.0) / 20.0);
    let (attack, release) = (secs_to_frames(num(c.p, "attack", 0.15), c.fps), secs_to_frames(num(c.p, "release", 0.4), c.fps));
    let ops: Vec<Op> = music.iter().map(|m| Op::SetProps { id: m.id.clone(), props: ClipPropsPatch { volume_keys: Some(duck_keys(&speech_frames, m, gain, attack, release)), ..ClipPropsPatch::default() } }).collect();
    let count = ops.len();
    commit(c.app, c.label, |e| {
        for op in &ops {
            e.apply(op)?;
        }
        Ok(())
    })?;
    Ok(done(count, format!("music lowered under {} speech parts", speech_frames.len())))
}

/// Инструмент-шаблон: операции таймлайна с подстановкой значений панели и свойств клипа.
fn run_steps(c: &Ctx<'_>, item: &Item) -> AppResult<ToolOutcome> {
    let steps = item.steps.as_array().ok_or_else(|| bad("no steps"))?;
    let mut vars: HashMap<String, Value> = c.p.as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v.clone())).collect();
    vars.insert("fps".into(), json!(c.fps));
    let clips: Vec<Option<&Clip>> = if item.each.as_deref() == Some("selection") { c.doc.timeline.clips.iter().filter(|x| c.ids.contains(&x.id)).map(Some).collect() } else { vec![c.doc.timeline.clips.iter().find(|x| c.ids.contains(&x.id))] };
    if item.each.as_deref() == Some("selection") && clips.is_empty() {
        return Err(bad("select clips first"));
    }
    let mut plan: Vec<(Value, Option<AppliedEffect>)> = Vec::new();
    for clip in clips {
        let mut local = vars.clone();
        if let Some(x) = clip {
            local.insert("clip.id".into(), json!(x.id));
            local.insert("clip.track".into(), json!(x.track));
            local.insert("clip.start".into(), json!(x.start));
            local.insert("clip.end".into(), json!(x.end()));
            local.insert("clip.duration".into(), json!(x.duration));
        }
        for step in steps {
            let filled = fill(step, &local, "");
            // Псевдооперация: наложить эффект из библиотеки.
            let fx = if filled["op"] == "applyEffect" {
                let id = filled["effect"].as_str().ok_or_else(|| bad("applyEffect without effect"))?;
                let (it, _) = library::find(&library::root_of(c.app)?, id).ok_or_else(|| bad(format!("effect {id} not found")))?;
                Some(library::applied(&it, &filled["params"]))
            } else {
                None
            };
            plan.push((filled, fx));
        }
    }
    let count = plan.len();
    commit(c.app, c.label, |e| {
        for (raw, fx) in &plan {
            let op = match fx {
                Some(effect) => Op::AddEffect { clip: raw["clip"].as_str().unwrap_or("").to_owned(), effect: effect.clone() },
                None => serde_json::from_value(raw.clone()).map_err(|er| bad(format!("step {raw}: {er}")))?,
            };
            e.apply(&op)?;
        }
        Ok(())
    })?;
    Ok(done(count, format!("{count} steps applied")))
}

/// Запустить инструмент. `wait` — сколько ждать долгую расшифровку (дальше — `pending`).
pub fn run(app: &AppHandle, id: &str, params: &Value, clips: &[String], label: &str, wait: Duration) -> AppResult<ToolOutcome> {
    let root = library::root_of(app)?;
    library::init(&root)?;
    let (item, _) = library::find(&root, id).ok_or_else(|| bad(format!("tool {id} not found")))?;
    if item.kind != "tool" {
        return Err(bad(format!("{id} is an effect: apply it to a clip")));
    }
    let (dir, doc) = app.state::<ProjectState>().context().ok_or_else(|| AppError::new("noProject", "no project is open"))?;
    let p = library::with_defaults(&item.controls, params);
    let fps = doc.project.sequence.fps;
    let ffmpeg = find(app, ToolId::Ffmpeg).ok_or_else(|| AppError::new("ffmpegMissing", "ffmpeg not found"))?;
    let c = Ctx { app, dir: &dir, doc: &doc, p: &p, ids: clips, label, fps, ffmpeg };
    let result = match (item.engine.as_deref(), item.builtin.as_deref()) {
        (Some("ops"), _) => run_steps(&c, &item),
        (_, Some("beatCut")) => beat_cut(&c),
        (_, Some("removeSilence")) => remove_silence(&c),
        (_, Some("captionsFromSpeech")) => captions(&c, wait),
        (_, Some("normalize")) => normalize(&c),
        (_, Some("duck")) => duck(&c),
        _ => Err(bad(format!("{id}: unknown engine"))),
    }?;
    if result.status == "done" {
        library::bump(&app.state::<Db>(), id);
        library::changed(app);
    }
    log::info!("tool {id}: {} ({})", result.status, result.note);
    Ok(result)
}

/// Наложить эффект библиотеки на клипы одним шагом отмены.
pub fn apply_effect(app: &AppHandle, id: &str, params: &Value, clips: &[String], label: &str) -> AppResult<Vec<String>> {
    let root = library::root_of(app)?;
    library::init(&root)?;
    let (item, _) = library::find(&root, id).ok_or_else(|| AppError::new("invalidLibrary", format!("{id} not found")))?;
    if item.kind != "effect" {
        return Err(AppError::new("invalidLibrary", format!("{id} is a tool: run it")));
    }
    if clips.is_empty() {
        return Err(AppError::new("invalidEdit", "no clips"));
    }
    let fx = library::applied(&item, params);
    let mut made = Vec::new();
    commit(app, label, |e| {
        for c in clips {
            made.extend(e.apply(&Op::AddEffect { clip: c.clone(), effect: fx.clone() })?);
        }
        Ok(())
    })?;
    library::bump(&app.state::<Db>(), id);
    library::changed(app);
    Ok(made)
}

#[tauri::command]
#[specta::specta]
pub async fn tool_run(app: AppHandle, id: String, params: Json, clips: Vec<String>) -> AppResult<ToolOutcome> {
    let label = format!("tool:{id}");
    tauri::async_runtime::spawn_blocking(move || run(&app, &id, &params.0, &clips, &label, Duration::from_secs(1800))).await.map_err(|e| AppError::new("internal", e.to_string()))?
}

#[tauri::command]
#[specta::specta]
pub fn effect_apply(app: AppHandle, id: String, params: Json, clips: Vec<String>) -> AppResult<Vec<String>> {
    apply_effect(&app, &id, &params.0, &clips, "effect")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::timeline::{ClipProps, Marker, Track, default_tracks};

    fn clip(id: &str, track: &str, start: u32, duration: u32, offset: u32) -> Clip {
        Clip { id: id.into(), track: track.into(), kind: ClipKind::Media, src: Some("media/voice/a.wav".into()), text: None, start, duration, offset, link: None, props: ClipProps::default(), effects: vec![] }
    }

    #[test]
    fn segments_fill_the_range_without_gaps() {
        let s = beat_segments(&[30, 60, 62, 90, 500], 10, 120, 1, 5);
        assert_eq!(s.first().unwrap().0, 10);
        assert_eq!(s.iter().map(|x| x.1).sum::<u32>(), 110, "кадры не потеряны");
        assert!(s.windows(2).all(|w| w[0].0 + w[0].1 == w[1].0), "встык: {s:?}");
        assert!(s.iter().all(|x| x.1 >= 5), "слишком короткий кусок присоединен");
        assert_eq!(beat_segments(&[30, 60, 90], 0, 120, 2, 1), vec![(0, 30), (30, 60), (90, 30)], "каждый второй удар");
        assert_eq!(beat_segments(&[], 0, 100, 1, 1), vec![(0, 100)], "без ударов — один кусок");
    }

    #[test]
    fn beat_clips_cycle_and_stay_inside_file() {
        let sources = [Source { src: "media/footage/a.mp4".into(), offset: 10, frames: Some(100) }, Source { src: "media/images/b.png".into(), offset: 0, frames: None }];
        let segs = [(0, 40), (40, 40), (80, 40), (120, 40), (160, 40), (200, 40)];
        let ops = beat_clips(&segs, &sources, false, "v1");
        let got: Vec<(String, u32)> = ops.iter().map(|o| match o { Op::AddClip { src, offset, .. } => (src.clone().unwrap(), offset.unwrap()), _ => panic!() }).collect();
        assert_eq!(got[0], ("media/footage/a.mp4".to_owned(), 10));
        assert_eq!(got[1].0, "media/images/b.png");
        assert_eq!(got[2], ("media/footage/a.mp4".to_owned(), 50), "следующий кусок — дальше по файлу");
        assert_eq!(got[4], ("media/footage/a.mp4".to_owned(), 10), "кончился файл — снова сначала");
        let mixed = beat_clips(&segs, &sources, true, "v1");
        let srcs: Vec<_> = mixed.iter().map(|o| match o { Op::AddClip { src, .. } => src.clone(), _ => None }).collect();
        assert!(srcs.windows(2).all(|w| w[0] != w[1]), "вперемешку без повторов подряд");
        assert_eq!(mixed, beat_clips(&segs, &sources, true, "v1"), "повторный запуск дает тот же монтаж");
    }

    #[test]
    fn silence_cuts_map_and_pad() {
        // Клип: кадр 100, с 30-го кадра файла (1 с при 30 к/с), длина 150 кадров (до 6 с файла).
        let c = clip("c1", "a1", 100, 150, 30);
        let cuts = silence_cuts(&c, &[(0.0, 2.0), (3.0, 4.0), (5.0, 5.1), (5.5, 9.0)], 30, 0.1, 2);
        // 0–2 с: от края клипа до 60−3 кадра; 3–4 с: с запасом; короткая после запаса — нет; 5.5 с —
        // до конца клипа вплотную.
        assert_eq!(cuts, vec![(100, 127), (163, 187), (238, 250)]);
    }

    #[test]
    fn cuts_ripple_clip_links_and_captions() {
        let mut tracks: Vec<Track> = default_tracks();
        let mut markers: Vec<Marker> = vec![];
        let mut a = clip("c1", "a1", 0, 300, 0);
        a.link = Some("l1".into());
        let mut v = clip("c2", "v1", 0, 300, 0);
        v.link = Some("l1".into());
        let mut cap = clip("c3", "s1", 200, 30, 0);
        cap.kind = ClipKind::Caption;
        cap.src = None;
        cap.text = Some("текст".into());
        let mut clips = vec![a, v, cap];
        let mut e = Edit { tracks: &mut tracks, clips: &mut clips, markers: &mut markers };
        apply_cuts(&mut e, "c1", &[(0, 30), (100, 150), (280, 300)]).unwrap();
        let audio: Vec<(u32, u32, u32)> = {
            let mut x: Vec<_> = clips.iter().filter(|c| c.track == "a1").map(|c| (c.start, c.duration, c.offset)).collect();
            x.sort_unstable();
            x
        };
        // Первый вырез с начала, второй в середине, третий в конце: без дыр, файл идет дальше.
        assert_eq!(audio, vec![(0, 70, 30), (70, 130, 150)]);
        assert_eq!(clips.iter().filter(|c| c.track == "v1").count(), 2, "картинка режется вместе со звуком");
        assert_eq!(clips.iter().find(|c| c.track == "s1").unwrap().start, 120, "субтитр едет с речью (200 − 50 − 30)");
    }

    #[test]
    fn ducking_uses_file_frames_and_merges_short_gaps() {
        let c = clip("m", "a2", 100, 1000, 500);
        let keys = duck_keys(&[(200, 300), (305, 400), (800, 900)], &c, 0.25, 10, 20);
        assert_eq!(keys.len(), 8, "короткая пауза склеена: {keys:?}");
        assert_eq!(keys[0], (590, 1.0), "в кадрах файла: 200 − 100 + 500 − 10");
        assert_eq!(keys[1], (600, 0.25));
        assert_eq!(keys[2], (800, 0.25));
        assert_eq!(keys[3], (820, 1.0));
        assert!(keys.windows(2).all(|w| w[0].0 < w[1].0), "по возрастанию");
        assert!(duck_keys(&[(0, 50)], &c, 0.25, 10, 20).is_empty(), "речь вне клипа");
    }

    #[test]
    fn templates_fill_values() {
        let vars: HashMap<String, Value> = [("clip.start".to_owned(), json!(100)), ("clip.id".to_owned(), json!("c7")), ("seconds".to_owned(), json!(1.5)), ("fps".to_owned(), json!(30))].into_iter().collect();
        assert_eq!(eval("$clip.id", &vars), Some(json!("c7")));
        assert_eq!(eval("$seconds*$fps", &vars), Some(json!(45.0)));
        assert_eq!(eval("$clip.start+$seconds*$fps", &vars), Some(json!(3045.0)), "слева направо");
        assert_eq!(eval("$nope", &vars), None);
        let t = json!({ "op": "splitClip", "id": "$clip.id", "at": "$clip.start+$seconds" });
        let f = fill(&t, &vars, "");
        assert_eq!(f, json!({ "op": "splitClip", "id": "c7", "at": 102 }), "кадр — целое число");
        let op: Op = serde_json::from_value(f).unwrap();
        assert_eq!(op, Op::SplitClip { id: "c7".into(), at: 102 });
    }
}
