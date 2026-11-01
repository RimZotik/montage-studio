//! Распознавание речи (`docs/extensions.md`): локально, встроенным фильтром whisper в ffmpeg и
//! моделью-компонентом. Итог — слова со временем в кэше проекта (`.studio/cache/speech/`), повторно
//! не считается. Расшифровка идет фоном: долгий файл не держит ни интерфейс, ни Claude.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::checks::{ToolId, components_dir, find};
use crate::error::{AppError, AppResult};
use crate::media::quiet;
use crate::project::timeline::{Clip, ClipKind, Op};

const CACHE_VERSION: u32 = 1;

/// Слово: время от начала файла, миллисекунды.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Word {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transcript {
    pub version: u32,
    pub language: String,
    pub words: Vec<Word>,
}

/// Строки JSON фильтра whisper → слова (служебные метки вроде `[Music]` отброшены).
pub fn parse_words(raw: &str) -> Vec<Word> {
    raw.lines()
        .filter_map(|l| serde_json::from_str::<Word>(l).ok())
        .map(|w| Word { text: w.text.trim().to_owned(), ..w })
        .filter(|w| !w.text.is_empty() && !w.text.starts_with(['[', '(', '♪']))
        .collect()
}

/// Фраза из нескольких слов.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// Короткие слова, которые по смыслу держатся за следующее: «у нас», «в доме», "to go".
const GLUE: &[&str] = &["а", "в", "во", "и", "к", "ко", "о", "об", "обо", "от", "у", "с", "со", "до", "за", "из", "на", "над", "не", "ни", "но", "по", "под", "при", "про", "что", "как", "для", "без", "да", "a", "an", "the", "to", "of", "in", "on", "at", "by", "for", "and", "or", "but", "not", "is", "i"];

fn glue(word: &str) -> bool {
    GLUE.contains(&word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()))
}

fn ends_sentence(word: &str) -> bool {
    word.ends_with(['.', '!', '?', '…'])
}

/// Собрать слова во фразы: новая — после конца предложения, паузы длиннее `gap_ms`, при `max_words`
/// словах или `max_chars` знаках. Предлог или союз не остается один и не закрывает фразу: уходит к
/// следующему слову («у нас» — вместе даже при одном слове на экране).
pub fn phrases(words: &[Word], max_words: usize, max_chars: usize, gap_ms: u32) -> Vec<Phrase> {
    let length = |g: &[&Word]| g.iter().map(|w| w.text.chars().count() + 1).sum::<usize>();
    let mut groups: Vec<Vec<&Word>> = Vec::new();
    for w in words {
        let (mut joins, mut moved) = (false, None);
        if let Some(g) = groups.last_mut()
            && let Some(&prev) = g.last() {
                let near = !ends_sentence(&prev.text) && w.start.saturating_sub(prev.end) <= gap_ms;
                joins = near && length(g) + w.text.chars().count() <= max_chars.max(1) && (g.len() < max_words.max(1) || glue(&prev.text));
                if !joins && near && g.len() > 1 && glue(&prev.text) {
                    moved = g.pop();
                }
            }
        match groups.last_mut() {
            Some(g) if joins => g.push(w),
            _ => groups.push(moved.into_iter().chain(std::iter::once(w)).collect()),
        }
    }
    groups
        .into_iter()
        .filter_map(|g| Some(Phrase { start: g.first()?.start, end: g.last()?.end, text: g.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ") }))
        .collect()
}

/// Быстрая речь: фраза, которую не успеть прочитать за `min_ms`, объединяется со следующей, пока
/// влезает в строку. Так субтитры поспевают за голосом, а не копят отставание.
pub fn merge_fast(list: Vec<Phrase>, min_ms: u32, max_chars: usize, gap_ms: u32) -> Vec<Phrase> {
    let mut out: Vec<Phrase> = Vec::new();
    for p in list {
        match out.last_mut() {
            Some(l) if l.end.saturating_sub(l.start) < min_ms && !ends_sentence(&l.text) && p.start.saturating_sub(l.end) <= gap_ms && l.text.chars().count() + 1 + p.text.chars().count() <= max_chars.max(1) => {
                l.text.push(' ');
                l.text.push_str(&p.text);
                l.end = p.end;
            }
            _ => out.push(p),
        }
    }
    out
}

/// Сколько текста влезает в строку кадра: знаков по ширине и размеру шрифта, слов — по знакам.
pub fn line_limits(width: u32, font_px: f64) -> (usize, usize) {
    let chars = ((f64::from(width) * 0.84 / (font_px.max(8.0) * 0.55)) as usize).clamp(8, 90);
    (chars, (chars / 6).clamp(3, 10))
}

/// Время файла (мс) → кадр таймлайна у этого клипа; `None` — обрезано и не звучит.
pub fn frame_on_clip(clip: &Clip, ms: u32, fps: u32) -> Option<u32> {
    let source = (f64::from(ms) * f64::from(fps) / 1000.0).round();
    let inside = (source - f64::from(clip.offset)) / clip.props.speed;
    (inside >= 0.0 && inside < f64::from(clip.duration)).then(|| clip.start + inside.round() as u32)
}

/// Как собирать субтитры.
pub struct CaptionRules {
    pub max_words: usize,
    pub max_chars: usize,
    /// Слово на экране не меньше, секунд.
    pub min_hold_s: f64,
    /// Слова, которые не показывать.
    pub skip: Vec<String>,
}

/// Субтитры к клипу: по кадрам таймлайна, без наложений, короткая пауза между соседними закрыта.
pub fn caption_ops(clip: &Clip, words: &[Word], fps: u32, track: &str, rules: &CaptionRules) -> Vec<Op> {
    let CaptionRules { max_words, max_chars, min_hold_s, skip } = rules;
    let (max_words, max_chars) = (*max_words, *max_chars);
    let min = ((min_hold_s.max(0.05) * f64::from(fps)) as u32).max(1);
    let hold = fps * 3 / 10;
    // Частицы и предлоги можно не показывать: время речи остается у соседних слов.
    let kept: Vec<Word> = words.iter().filter(|w| !skip.iter().any(|s| s == &w.text.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_owned())).cloned().collect();
    let min_ms = (min_hold_s.max(0.05) * 1000.0) as u32;
    let timed: Vec<(u32, u32, String)> = merge_fast(phrases(&kept, max_words, max_chars, 700), min_ms, max_chars, 700)
        .into_iter()
        .filter_map(|p| {
            let a = frame_on_clip(clip, p.start, fps)?;
            let b = frame_on_clip(clip, p.end, fps).unwrap_or(clip.end());
            Some((a, b.max(a + 1), p.text))
        })
        .collect();
    let mut ops = Vec::new();
    // Реплика начинается ровно на своем слове: минимальная длина не сдвигает следующую, иначе
    // отставание копится к концу ролика.
    for (i, (start, b, text)) in timed.iter().enumerate() {
        let next = timed.get(i + 1).map_or(u32::MAX, |n| n.0);
        // Тянется до следующего, если между ними совсем мало.
        let held = if next.saturating_sub(*b) <= hold { next } else { (*b).max(start + min) };
        let end = held.min(next).min(clip.end()).max(start + 1);
        ops.push(Op::AddClip { track: track.to_owned(), kind: ClipKind::Caption, src: None, text: Some(text.clone()), start: *start, duration: end - start, offset: None });
    }
    ops
}

/// Ход расшифровки — интерфейсу.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct SpeechProgress {
    pub path: String,
    /// 0–1.
    pub progress: f64,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Clone)]
enum Job {
    Running(f64),
    Failed(String),
}

#[derive(Default)]
pub struct SpeechJobs(Mutex<HashMap<String, Job>>);

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ready(Transcript),
    Running(f64),
    NoModel,
}

pub fn model_path(app: &AppHandle) -> AppResult<PathBuf> {
    Ok(components_dir(app)?.join("speech").join(crate::components::SPEECH_MODEL))
}

/// Файл кэша: зависит от содержимого файла (путь, размер, время) и языка.
fn cache_file(dir: &Path, rel: &str, language: &str) -> AppResult<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(dir.join(rel))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (crate::media::key(rel, &meta), language).hash(&mut h);
    Ok(dir.join(".studio/cache/speech").join(format!("{:016x}.json", h.finish())))
}

fn language_ok(l: &str) -> bool {
    l == "auto" || (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase())
}

/// Готовая расшифровка, ход — или запуск фоном. Сбой прошлой попытки возвращается один раз:
/// следующий вызов пробует заново.
pub fn status_or_start(app: &AppHandle, dir: &Path, rel: &str, language: &str) -> AppResult<Status> {
    if !language_ok(language) {
        return Err(AppError::new("invalidEdit", format!("language {language}")));
    }
    let cache = cache_file(dir, rel, language)?;
    if let Some(t) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice::<Transcript>(&b).ok()).filter(|t| t.version == CACHE_VERSION) {
        return Ok(Status::Ready(t));
    }
    let model = model_path(app)?;
    if !model.is_file() {
        return Ok(Status::NoModel);
    }
    let jobs = app.state::<SpeechJobs>();
    let mut map = jobs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = cache.display().to_string();
    match map.get(&key).cloned() {
        Some(Job::Running(p)) => return Ok(Status::Running(p)),
        Some(Job::Failed(e)) => {
            map.remove(&key);
            return Err(AppError::new("speechFailed", e));
        }
        None => {}
    }
    map.insert(key.clone(), Job::Running(0.0));
    let (app2, file, rel2, lang) = (app.clone(), dir.join(rel), rel.to_owned(), language.to_owned());
    std::thread::spawn(move || {
        // Сбой внутри не оставляет расшифровку «идущей» навсегда.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            transcribe(&app2, &file, &rel2, &lang, &model, &key).and_then(|t| {
                if let Some(parent) = cache.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                crate::project::write_atomic(&cache, &serde_json::to_vec(&t).map_err(|e| AppError::new("internal", e.to_string()))?)
            })
        }))
        .unwrap_or_else(|_| Err(AppError::new("speechFailed", "internal error")));
        let jobs = app2.state::<SpeechJobs>();
        let mut map = jobs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let error = result.err().map(|e| e.message);
        match &error {
            Some(e) => {
                log::warn!("speech failed for {rel2}: {e}");
                map.insert(key, Job::Failed(e.clone()));
            }
            None => {
                log::info!("speech done for {rel2}");
                map.remove(&key);
            }
        }
        drop(map);
        let _ = SpeechProgress { path: rel2, progress: 1.0, done: true, error }.emit(&app2);
    });
    Ok(Status::Running(0.0))
}

/// Дождаться расшифровки не дольше `wait`.
pub fn wait(app: &AppHandle, dir: &Path, rel: &str, language: &str, wait: std::time::Duration) -> AppResult<Status> {
    let started = std::time::Instant::now();
    loop {
        let status = status_or_start(app, dir, rel, language)?;
        if !matches!(status, Status::Running(_)) || started.elapsed() >= wait {
            return Ok(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

fn transcribe(app: &AppHandle, file: &Path, rel: &str, language: &str, model: &Path, key: &str) -> AppResult<Transcript> {
    let ffmpeg = find(app, ToolId::Ffmpeg).ok_or_else(|| AppError::new("ffmpegMissing", "ffmpeg not found"))?;
    let duration = find(app, ToolId::Ffprobe).and_then(|p| crate::media::probe(&p, file)).and_then(|p| p.duration_s).unwrap_or(0.0);
    let folder = model.parent().ok_or_else(|| AppError::new("internal", "model path"))?;
    // Пути в параметрах фильтра ломаются на двоеточиях и кириллице — модель и итог рядом, папка
    // запуска — их папка.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let out_name = format!("job-{}-{}.json", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let out_file = folder.join(&out_name);
    let _ = std::fs::remove_file(&out_file);
    let filter = format!("whisper=model={}:language={language}:queue=20:format=json:max_len=1:destination={out_name}", crate::components::SPEECH_MODEL);
    let mut c = Command::new(&ffmpeg);
    c.current_dir(folder).args(["-hide_banner", "-nostats", "-y", "-i"]).arg(file).args(["-vn", "-af", &filter, "-f", "null", "-"]).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = quiet(&mut c).spawn().map_err(|e| AppError::new("ffmpeg", e.to_string()))?;
    let mut last = String::new();
    if let Some(err) = child.stderr.take() {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if let Some(ms) = line.split("run transcription at ").nth(1).and_then(|r| r.split(' ').next()).and_then(|n| n.parse::<f64>().ok()) {
                let progress = if duration > 0.0 { (ms / 1000.0 / duration).clamp(0.0, 0.99) } else { 0.0 };
                app.state::<SpeechJobs>().0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key.to_owned(), Job::Running(progress));
                let _ = SpeechProgress { path: rel.to_owned(), progress, done: false, error: None }.emit(app);
            }
            if !line.trim().is_empty() {
                last = line;
            }
        }
    }
    let ok = child.wait().map_err(|e| AppError::new("ffmpeg", e.to_string()))?.success();
    let raw = std::fs::read_to_string(&out_file).unwrap_or_default();
    let _ = std::fs::remove_file(&out_file);
    if !ok {
        return Err(AppError::new("speechFailed", last));
    }
    Ok(Transcript { version: CACHE_VERSION, language: language.to_owned(), words: parse_words(&raw) })
}

/// Все готовые расшифровки проекта: поиск слов не запускает ничего нового.
pub fn cached_for(dir: &Path, rel: &str) -> Option<Transcript> {
    ["auto", "ru", "en"].iter().find_map(|l| std::fs::read(cache_file(dir, rel, l).ok()?).ok().and_then(|b| serde_json::from_slice(&b).ok()))
}

/// Установлена ли модель распознавания и какого она размера (для кнопки установки).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModel {
    pub installed: bool,
    pub download_mb: u32,
}

#[tauri::command]
#[specta::specta]
pub fn speech_model(app: AppHandle) -> AppResult<SpeechModel> {
    let size = crate::components::manifest(crate::components::ComponentId::Speech).size;
    Ok(SpeechModel { installed: model_path(&app)?.is_file(), download_mb: u32::try_from(size.div_ceil(1 << 20)).unwrap_or(0) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::timeline::{ClipProps, Op};

    fn word(start: u32, end: u32, text: &str) -> Word {
        Word { start, end, text: text.into() }
    }

    fn clip(start: u32, duration: u32, offset: u32) -> Clip {
        Clip { id: "c1".into(), track: "a1".into(), kind: ClipKind::Media, src: Some("media/voice/a.wav".into()), text: None, start, duration, offset, link: None, props: ClipProps::default(), effects: vec![] }
    }

    #[test]
    fn slow_clip_stretches_time() {
        let mut c = clip(100, 100, 30);
        c.props.speed = 0.5;
        assert_eq!(frame_on_clip(&c, 2000, 30), Some(160), "секунда файла идет две секунды");
        assert_eq!(frame_on_clip(&c, 900, 30), None);
    }

    #[test]
    fn filter_output_is_parsed() {
        let raw = "{\"start\":0,\"end\":130,\"text\":\" Я\"}\n{\"start\":130,\"end\":450,\"text\":\"[Music]\"}\nмусор\n{\"start\":450,\"end\":990,\"text\":\"модель,\"}\n";
        assert_eq!(parse_words(raw), vec![word(0, 130, "Я"), word(450, 990, "модель,")]);
    }

    #[test]
    fn phrases_break_on_sentence_gap_and_limits() {
        let words = [word(0, 100, "раз"), word(100, 200, "два"), word(200, 300, "три."), word(300, 400, "еще"), word(2000, 2100, "после")];
        let by_rules: Vec<String> = phrases(&words, 10, 80, 700).into_iter().map(|p| p.text).collect();
        assert_eq!(by_rules, ["раз два три.", "еще", "после"]);
        assert_eq!(phrases(&words, 2, 80, 700)[0].text, "раз два");
        assert_eq!(phrases(&words, 10, 7, 700)[0].text, "раз два", "ограничение по знакам");
    }

    #[test]
    fn prepositions_stay_with_the_next_word() {
        let words = [word(0, 100, "у"), word(100, 300, "нас"), word(300, 500, "большой"), word(500, 700, "дом"), word(700, 800, "в"), word(800, 1000, "лесу")];
        let one: Vec<String> = phrases(&words, 1, 80, 700).into_iter().map(|p| p.text).collect();
        assert_eq!(one, ["у нас", "большой", "дом", "в лесу"], "даже по одному слову предлог не один");
        let two: Vec<String> = phrases(&words, 3, 80, 700).into_iter().map(|p| p.text).collect();
        assert_eq!(two, ["у нас большой", "дом в лесу"]);
        let narrow: Vec<String> = phrases(&words, 9, 14, 700).into_iter().map(|p| p.text).collect();
        assert_eq!(narrow, ["у нас большой", "дом в лесу"], "по ширине предлог уходит на следующую строку");
    }

    #[test]
    fn fast_speech_does_not_lag_behind_the_voice() {
        // 20 слов по 0,1 с: при минимуме 0,4 с прежняя схема отставала бы на секунды.
        let words: Vec<Word> = (0..20).map(|i| word(i * 100, (i + 1) * 100, "слово")).collect();
        let c = clip(0, 300, 0);
        let ops = caption_ops(&c, &words, 30, "s1", &CaptionRules { max_words: 1, max_chars: 30, min_hold_s: 0.4, skip: vec![] });
        let got: Vec<(u32, u32, String)> = ops.iter().map(|o| match o { Op::AddClip { start, duration, text, .. } => (*start, *duration, text.clone().unwrap()), _ => panic!() }).collect();
        assert!(got.len() < 20, "быстрая речь объединяется: {got:?}");
        assert!(got.iter().all(|x| x.2.split(' ').count() >= 2 || x == got.last().unwrap()));
        let last = got.last().unwrap();
        assert!(last.0 <= 60, "последняя реплика на своем слове, без отставания: {last:?}");
        for w in got.windows(2) {
            assert!(w[0].0 + w[0].1 <= w[1].0, "без наложений");
        }
    }

    #[test]
    fn wide_frame_fits_a_longer_line() {
        let (vertical, _) = line_limits(1080, 64.0);
        let (wide, words) = line_limits(1920, 64.0);
        assert!((20..=30).contains(&vertical));
        assert!(wide > vertical + 15 && words >= 6);
    }

    #[test]
    fn captions_follow_trim_and_do_not_overlap() {
        // Клип начинается со второй секунды файла (offset 60 при 30 к/с) и стоит с кадра 300.
        let c = clip(300, 90, 60);
        let words = [word(500, 900, "обрезано"), word(2000, 2500, "первое."), word(2600, 3000, "второе."), word(5000, 5300, "конец")];
        let ops = caption_ops(&c, &words, 30, "s1", &CaptionRules { max_words: 3, max_chars: 30, min_hold_s: 0.25, skip: vec![] });
        let got: Vec<(u32, u32, String)> = ops.iter().map(|o| match o { Op::AddClip { start, duration, text, .. } => (*start, *duration, text.clone().unwrap()), _ => panic!() }).collect();
        assert_eq!(got.len(), 2, "слово до начала клипа и слово после конца — не показываются: {got:?}");
        assert_eq!(got[0].0, 300);
        assert_eq!(got[0].0 + got[0].1, got[1].0, "короткая пауза закрыта, без наложения");
        assert_eq!(got[1].2, "второе.");
    }

    #[test]
    fn captions_hold_short_words_and_skip_listed() {
        let c = clip(0, 300, 0);
        let words = [word(0, 50, "а"), word(100, 150, "да."), word(3000, 3050, "нет.")];
        let ops = caption_ops(&c, &words, 30, "s1", &CaptionRules { max_words: 1, max_chars: 30, min_hold_s: 0.5, skip: vec!["а".to_owned()] });
        let got: Vec<(u32, String)> = ops.iter().map(|o| match o { Op::AddClip { duration, text, .. } => (*duration, text.clone().unwrap()), _ => panic!() }).collect();
        assert_eq!(got, vec![(15, "да.".to_owned()), (15, "нет.".to_owned())], "короткое слово держится, «а» опущено");
    }

    #[test]
    fn language_is_checked_before_it_reaches_the_filter() {
        assert!(language_ok("ru") && language_ok("auto"));
        assert!(!language_ok("ru:model=x") && !language_ok(""));
    }
}
