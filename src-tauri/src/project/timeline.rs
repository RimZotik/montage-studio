//! Таймлайн (`docs/project-format.md`): дорожки и клипы, время в кадрах. Все правки — пакетами
//! операций (`Op`) через сервис проекта: пакет проверяется и применяется целиком, это один шаг
//! истории. Теми же операциями будет править Claude (инструменты MCP студии).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrackKind {
    Video,
    Audio,
    Captions,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub kind: TrackKind,
    pub name: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ClipKind {
    /// Файл медиа (видео, звук, картинка).
    Media,
    /// Текст субтитров.
    Caption,
    /// Заливка цветом.
    Solid,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClipProps {
    /// Громкость 0–2 (1 — как есть).
    #[serde(default = "one")]
    pub volume: f64,
    /// Прозрачность 0–1.
    #[serde(default = "one")]
    pub opacity: f64,
    /// Масштаб (1 — по кадру).
    #[serde(default = "one")]
    pub scale: f64,
    /// Сдвиг от центра кадра, пикселей последовательности.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// Плавное появление и исчезновение звука/картинки, кадров.
    #[serde(default)]
    pub fade_in: u32,
    #[serde(default)]
    pub fade_out: u32,
}

fn one() -> f64 {
    1.0
}

impl Default for ClipProps {
    fn default() -> Self {
        Self { volume: 1.0, opacity: 1.0, scale: 1.0, x: 0.0, y: 0.0, fade_in: 0, fade_out: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    pub track: String,
    pub kind: ClipKind,
    /// Путь к файлу внутри проекта (`media/...`) — у `media`.
    #[serde(default)]
    pub src: Option<String>,
    /// Текст — у `caption`; цвет `#RRGGBB` — у `solid`.
    #[serde(default)]
    pub text: Option<String>,
    /// Начало на таймлайне, кадр.
    pub start: u32,
    /// Длительность, кадров.
    pub duration: u32,
    /// С какого кадра файла начинается клип (обрезка начала).
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub props: ClipProps,
}

impl Clip {
    pub fn end(&self) -> u32 {
        self.start + self.duration
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub frame: u32,
    #[serde(default)]
    pub label: String,
}

/// Операции правки таймлайна (пакет — один шаг истории).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "op")]
pub enum Op {
    AddClip { track: String, kind: ClipKind, src: Option<String>, text: Option<String>, start: u32, duration: u32, offset: Option<u32> },
    MoveClip { id: String, track: Option<String>, start: u32 },
    /// Новые границы: `start`/`duration` на таймлайне и `offset` в файле (что не задано — как было).
    TrimClip { id: String, start: Option<u32>, duration: Option<u32>, offset: Option<u32> },
    /// Разрезать клип в кадре таймлайна.
    SplitClip { id: String, at: u32 },
    /// Удалить клипы; `ripple` — сдвинуть следующие клипы тех же дорожек влево на освободившееся.
    DeleteClips { ids: Vec<String>, ripple: bool },
    SetProps { id: String, props: ClipProps },
    SetText { id: String, text: String },
    SetTrack { id: String, muted: Option<bool>, locked: Option<bool> },
    AddMarker { frame: u32, label: String },
    RemoveMarker { frame: u32 },
}

/// Стандартные дорожки нового проекта: две видео, субтитры, голос, музыка, звуки.
pub fn default_tracks() -> Vec<Track> {
    let t = |id: &str, kind, name: &str| Track { id: id.into(), kind, name: name.into(), muted: false, locked: false };
    vec![t("v2", TrackKind::Video, "V2"), t("v1", TrackKind::Video, "V1"), t("s1", TrackKind::Captions, "S1"), t("a1", TrackKind::Audio, "A1"), t("a2", TrackKind::Audio, "A2"), t("a3", TrackKind::Audio, "A3")]
}

/// Свободный ID клипа.
fn next_id(clips: &[Clip]) -> String {
    let max = clips.iter().filter_map(|c| c.id.strip_prefix('c').and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0);
    format!("c{}", max + 1)
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::new("invalidEdit", msg)
}

/// Таймлайн для операций.
pub struct Edit<'a> {
    pub tracks: &'a mut Vec<Track>,
    pub clips: &'a mut Vec<Clip>,
    pub markers: &'a mut Vec<Marker>,
}

impl Edit<'_> {
    fn clip_mut(&mut self, id: &str) -> AppResult<&mut Clip> {
        self.clips.iter_mut().find(|c| c.id == id).ok_or_else(|| bad(format!("clip {id} not found")))
    }

    fn track(&self, id: &str) -> AppResult<&Track> {
        self.tracks.iter().find(|t| t.id == id).ok_or_else(|| bad(format!("track {id} not found")))
    }

    fn unlocked(&self, track: &str) -> AppResult<()> {
        if self.track(track)?.locked {
            return Err(AppError::new("trackLocked", track.to_owned()));
        }
        Ok(())
    }

    /// Применить операцию. Возвращает ID нового клипа (у `AddClip`, `SplitClip`).
    pub fn apply(&mut self, op: &Op) -> AppResult<Option<String>> {
        match op {
            Op::AddClip { track, kind, src, text, start, duration, offset } => {
                self.unlocked(track)?;
                let id = next_id(self.clips);
                self.clips.push(Clip { id: id.clone(), track: track.clone(), kind: *kind, src: src.clone(), text: text.clone(), start: *start, duration: *duration, offset: offset.unwrap_or(0), props: ClipProps::default() });
                Ok(Some(id))
            }
            Op::MoveClip { id, track, start } => {
                let from = self.clip_mut(id)?.track.clone();
                self.unlocked(&from)?;
                if let Some(t) = track {
                    self.unlocked(t)?;
                }
                let c = self.clip_mut(id)?;
                c.start = *start;
                if let Some(t) = track {
                    c.track = t.clone();
                }
                Ok(None)
            }
            Op::TrimClip { id, start, duration, offset } => {
                let track = self.clip_mut(id)?.track.clone();
                self.unlocked(&track)?;
                let c = self.clip_mut(id)?;
                if let Some(s) = start {
                    c.start = *s;
                }
                if let Some(d) = duration {
                    c.duration = *d;
                }
                if let Some(o) = offset {
                    c.offset = *o;
                }
                Ok(None)
            }
            Op::SplitClip { id, at } => {
                let c = self.clip_mut(id)?.clone();
                self.unlocked(&c.track)?;
                if *at <= c.start || *at >= c.end() {
                    return Err(bad(format!("split {at} outside clip {id}")));
                }
                let left = at - c.start;
                let new_id = next_id(self.clips);
                let mut right = c.clone();
                right.id = new_id.clone();
                right.start = *at;
                right.duration = c.duration - left;
                if c.kind == ClipKind::Media {
                    right.offset = c.offset + left;
                }
                right.props.fade_in = 0;
                let first = self.clip_mut(id)?;
                first.duration = left;
                first.props.fade_out = 0;
                self.clips.push(right);
                Ok(Some(new_id))
            }
            Op::DeleteClips { ids, ripple } => {
                for id in ids {
                    let track = self.clip_mut(id)?.track.clone();
                    self.unlocked(&track)?;
                }
                let removed: Vec<Clip> = self.clips.iter().filter(|c| ids.contains(&c.id)).cloned().collect();
                self.clips.retain(|c| !ids.contains(&c.id));
                if *ripple {
                    // По дорожкам, с конца: каждый удаленный сдвигает то, что правее него.
                    let mut gone = removed;
                    gone.sort_by_key(|c| std::cmp::Reverse(c.start));
                    for r in gone {
                        for c in self.clips.iter_mut().filter(|c| c.track == r.track && c.start >= r.end()) {
                            c.start -= r.duration;
                        }
                    }
                }
                Ok(None)
            }
            Op::SetProps { id, props } => {
                self.clip_mut(id)?.props = props.clone();
                Ok(None)
            }
            Op::SetText { id, text } => {
                self.clip_mut(id)?.text = Some(text.clone());
                Ok(None)
            }
            Op::SetTrack { id, muted, locked } => {
                let t = self.tracks.iter_mut().find(|t| &t.id == id).ok_or_else(|| bad(format!("track {id} not found")))?;
                if let Some(m) = muted {
                    t.muted = *m;
                }
                if let Some(l) = locked {
                    t.locked = *l;
                }
                Ok(None)
            }
            Op::AddMarker { frame, label } => {
                self.markers.retain(|m| m.frame != *frame);
                self.markers.push(Marker { frame: *frame, label: label.clone() });
                self.markers.sort_by_key(|m| m.frame);
                Ok(None)
            }
            Op::RemoveMarker { frame } => {
                self.markers.retain(|m| m.frame != *frame);
                Ok(None)
            }
        }
    }
}

/// Проверка таймлайна: клипы на существующих дорожках подходящего вида, без наложений на одной
/// дорожке, длительность больше нуля, у медиа есть путь, у текста — текст.
pub fn validate(tracks: &[Track], clips: &[Clip]) -> AppResult<()> {
    for c in clips {
        let t = tracks.iter().find(|t| t.id == c.track).ok_or_else(|| bad(format!("clip {} on missing track {}", c.id, c.track)))?;
        let fits = matches!((t.kind, c.kind), (TrackKind::Captions, ClipKind::Caption) | (TrackKind::Video, ClipKind::Media | ClipKind::Solid) | (TrackKind::Audio, ClipKind::Media));
        if !fits {
            return Err(bad(format!("clip {} kind does not fit track {}", c.id, t.id)));
        }
        if c.duration == 0 {
            return Err(bad(format!("clip {} has zero duration", c.id)));
        }
        if c.kind == ClipKind::Media && !c.src.as_deref().is_some_and(|s| s.starts_with("media/") && !s.contains("..")) {
            return Err(bad(format!("clip {} has no media path", c.id)));
        }
        let p = &c.props;
        if !(0.0..=2.0).contains(&p.volume) || !(0.0..=1.0).contains(&p.opacity) || !(0.01..=20.0).contains(&p.scale) {
            return Err(bad(format!("clip {} props out of range", c.id)));
        }
    }
    let mut by_track: Vec<&Clip> = clips.iter().collect();
    by_track.sort_by(|a, b| a.track.cmp(&b.track).then(a.start.cmp(&b.start)));
    for pair in by_track.windows(2) {
        if pair[0].track == pair[1].track && pair[1].start < pair[0].end() {
            return Err(AppError::new("overlap", format!("{} and {} overlap on {}", pair[0].id, pair[1].id, pair[0].track)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(track: &str, start: u32, duration: u32) -> Op {
        Op::AddClip { track: track.into(), kind: ClipKind::Media, src: Some("media/footage/a.mp4".into()), text: None, start, duration, offset: None }
    }

    fn run(ops: &[Op]) -> (Vec<Track>, Vec<Clip>, AppResult<()>) {
        let (mut tracks, mut clips, mut markers) = (default_tracks(), vec![], vec![]);
        let mut e = Edit { tracks: &mut tracks, clips: &mut clips, markers: &mut markers };
        let mut result = Ok(());
        for op in ops {
            if let Err(err) = e.apply(op) {
                result = Err(err);
                break;
            }
        }
        let checked = result.and_then(|()| validate(&tracks, &clips));
        (tracks, clips, checked)
    }

    #[test]
    fn add_split_ripple() {
        let (_, clips, r) = run(&[media("v1", 0, 60), media("v1", 60, 30), Op::SplitClip { id: "c1".into(), at: 20 }, Op::DeleteClips { ids: vec!["c3".into()], ripple: true }]);
        r.unwrap();
        let mut v: Vec<(String, u32, u32, u32)> = clips.iter().map(|c| (c.id.clone(), c.start, c.duration, c.offset)).collect();
        v.sort();
        // c1 — 0..20, c3 (правая половина, 20..60, offset 20) удалена со сдвигом: c2 с 60 → 20.
        assert_eq!(v, vec![("c1".into(), 0, 20, 0), ("c2".into(), 20, 30, 0)]);
    }

    #[test]
    fn overlap_and_kinds_are_refused() {
        assert_eq!(run(&[media("v1", 0, 60), media("v1", 30, 60)]).2.unwrap_err().code, "overlap");
        let cap = Op::AddClip { track: "v1".into(), kind: ClipKind::Caption, src: None, text: Some("Hi".into()), start: 0, duration: 30, offset: None };
        assert_eq!(run(&[cap]).2.unwrap_err().code, "invalidEdit");
        assert!(run(&[media("v1", 0, 60), media("v2", 30, 60)]).2.is_ok(), "разные дорожки — можно");
    }

    #[test]
    fn locked_track_and_bad_split() {
        assert_eq!(run(&[Op::SetTrack { id: "v1".into(), muted: None, locked: Some(true) }, media("v1", 0, 10)]).2.unwrap_err().code, "trackLocked");
        assert_eq!(run(&[media("v1", 0, 10), Op::SplitClip { id: "c1".into(), at: 10 }]).2.unwrap_err().code, "invalidEdit");
    }

    #[test]
    fn ops_json_shape() {
        let op: Op = serde_json::from_str(r#"{"op":"moveClip","id":"c1","track":null,"start":12}"#).unwrap();
        assert_eq!(op, Op::MoveClip { id: "c1".into(), track: None, start: 12 });
    }
}
