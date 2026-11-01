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
    /// Группа связанных клипов (видео и звук одного файла): перенос, обрезка, разрез и удаление —
    /// вместе.
    #[serde(default)]
    pub link: Option<String>,
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
    /// Видео со звуком: клип картинки и связанный клип звука того же файла.
    AddLinked { video_track: String, audio_track: String, src: String, start: u32, duration: u32, offset: Option<u32> },
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

    /// ID клипов группы (связанные видео и звук одного файла), включая сам клип.
    fn group(&self, id: &str) -> AppResult<Vec<String>> {
        let c = self.clips.iter().find(|c| c.id == id).ok_or_else(|| bad(format!("clip {id} not found")))?;
        Ok(match &c.link {
            Some(l) => self.clips.iter().filter(|x| x.link.as_ref() == Some(l)).map(|x| x.id.clone()).collect(),
            None => vec![c.id.clone()],
        })
    }

    /// Поднять клипы наверх своих дорожек (позже поставленный — выше: перекрывает нижний, не
    /// обрезая его).
    fn raise(&mut self, ids: &[String]) {
        let (top, rest): (Vec<Clip>, Vec<Clip>) = self.clips.drain(..).partition(|c| ids.contains(&c.id));
        self.clips.extend(rest);
        self.clips.extend(top);
    }

    /// Применить операцию. Возвращает ID новых клипов (у `AddClip`, `AddLinked`, `SplitClip`).
    pub fn apply(&mut self, op: &Op) -> AppResult<Vec<String>> {
        match op {
            Op::AddClip { track, kind, src, text, start, duration, offset } => {
                self.unlocked(track)?;
                let id = next_id(self.clips);
                self.clips.push(Clip { id: id.clone(), track: track.clone(), kind: *kind, src: src.clone(), text: text.clone(), start: *start, duration: *duration, offset: offset.unwrap_or(0), link: None, props: ClipProps::default() });
                Ok(vec![id])
            }
            Op::AddLinked { video_track, audio_track, src, start, duration, offset } => {
                self.unlocked(video_track)?;
                self.unlocked(audio_track)?;
                let v = next_id(self.clips);
                let link = format!("l{}", v.trim_start_matches('c'));
                let base = Clip { id: v.clone(), track: video_track.clone(), kind: ClipKind::Media, src: Some(src.clone()), text: None, start: *start, duration: *duration, offset: offset.unwrap_or(0), link: Some(link), props: ClipProps::default() };
                self.clips.push(base.clone());
                let a = next_id(self.clips);
                self.clips.push(Clip { id: a.clone(), track: audio_track.clone(), ..base });
                Ok(vec![v, a])
            }
            Op::MoveClip { id, track, start } => {
                let group = self.group(id)?;
                let delta = i64::from(*start) - i64::from(self.clip_mut(id)?.start);
                for g in &group {
                    let t = self.clip_mut(g)?.track.clone();
                    self.unlocked(&t)?;
                }
                if let Some(t) = track {
                    self.unlocked(t)?;
                }
                for g in &group {
                    let c = self.clip_mut(g)?;
                    c.start = u32::try_from(i64::from(c.start) + delta).map_err(|_| bad("clip would start before 0"))?;
                }
                if let Some(t) = track {
                    self.clip_mut(id)?.track = t.clone();
                }
                self.raise(&group);
                Ok(vec![])
            }
            Op::TrimClip { id, start, duration, offset } => {
                let group = self.group(id)?;
                for g in &group {
                    let t = self.clip_mut(g)?.track.clone();
                    self.unlocked(&t)?;
                }
                for g in &group {
                    let c = self.clip_mut(g)?;
                    if let Some(s) = start {
                        c.start = *s;
                    }
                    if let Some(d) = duration {
                        c.duration = *d;
                    }
                    if let (Some(o), ClipKind::Media) = (offset, c.kind) {
                        c.offset = *o;
                    }
                }
                self.raise(&group);
                Ok(vec![])
            }
            Op::SplitClip { id, at } => {
                let group = self.group(id)?;
                let right_link = format!("l{}", next_id(self.clips).trim_start_matches('c'));
                let mut created = Vec::new();
                for g in &group {
                    let c = self.clip_mut(g)?.clone();
                    self.unlocked(&c.track)?;
                    if *at <= c.start || *at >= c.end() {
                        return Err(bad(format!("split {at} outside clip {g}")));
                    }
                    let left = at - c.start;
                    let mut right = c.clone();
                    right.id = next_id(self.clips);
                    right.start = *at;
                    right.duration = c.duration - left;
                    if c.kind == ClipKind::Media {
                        right.offset = c.offset + left;
                    }
                    right.props.fade_in = 0;
                    if c.link.is_some() {
                        right.link = Some(right_link.clone());
                    }
                    let first = self.clip_mut(g)?;
                    first.duration = left;
                    first.props.fade_out = 0;
                    created.push(right.id.clone());
                    self.clips.push(right);
                }
                Ok(created)
            }
            Op::DeleteClips { ids, ripple } => {
                let mut all: Vec<String> = Vec::new();
                for id in ids {
                    for g in self.group(id)? {
                        if !all.contains(&g) {
                            all.push(g);
                        }
                    }
                }
                for id in &all {
                    let track = self.clip_mut(id)?.track.clone();
                    self.unlocked(&track)?;
                }
                let removed: Vec<Clip> = self.clips.iter().filter(|c| all.contains(&c.id)).cloned().collect();
                self.clips.retain(|c| !all.contains(&c.id));
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
                Ok(vec![])
            }
            Op::SetProps { id, props } => {
                self.clip_mut(id)?.props = props.clone();
                Ok(vec![])
            }
            Op::SetText { id, text } => {
                self.clip_mut(id)?.text = Some(text.clone());
                Ok(vec![])
            }
            Op::SetTrack { id, muted, locked } => {
                let t = self.tracks.iter_mut().find(|t| &t.id == id).ok_or_else(|| bad(format!("track {id} not found")))?;
                if let Some(m) = muted {
                    t.muted = *m;
                }
                if let Some(l) = locked {
                    t.locked = *l;
                }
                Ok(vec![])
            }
            Op::AddMarker { frame, label } => {
                self.markers.retain(|m| m.frame != *frame);
                self.markers.push(Marker { frame: *frame, label: label.clone() });
                self.markers.sort_by_key(|m| m.frame);
                Ok(vec![])
            }
            Op::RemoveMarker { frame } => {
                self.markers.retain(|m| m.frame != *frame);
                Ok(vec![])
            }
        }
    }
}

/// Проверка таймлайна: клипы на существующих дорожках подходящего вида, длительность больше
/// нуля, у медиа есть путь. Наложение на одной дорожке разрешено: верхний (позже в списке)
/// перекрывает нижний, не обрезая его.
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
        assert!(run(&[media("v1", 0, 60), media("v1", 30, 60)]).2.is_ok(), "наложение не обрезает нижний");
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
    fn linked_move_split_delete() {
        let add = Op::AddLinked { video_track: "v1".into(), audio_track: "a1".into(), src: "media/footage/a.mp4".into(), start: 0, duration: 100, offset: None };
        let (_, clips, r) = run(&[add.clone(), Op::MoveClip { id: "c1".into(), track: None, start: 30 }]);
        r.unwrap();
        assert!(clips.iter().all(|c| c.start == 30), "звук едет вместе с видео");
        let (_, clips, r) = run(&[add.clone(), Op::SplitClip { id: "c2".into(), at: 40 }]);
        r.unwrap();
        assert_eq!(clips.len(), 4, "разрез — у обоих");
        let right: Vec<_> = clips.iter().filter(|c| c.start == 40).collect();
        assert!(right.iter().all(|c| c.link == right[0].link && c.offset == 40), "правые части связаны между собой");
        let (_, clips, r) = run(&[add, Op::DeleteClips { ids: vec!["c1".into()], ripple: false }]);
        r.unwrap();
        assert!(clips.is_empty(), "удаление — вместе");
    }

    #[test]
    fn moved_clip_goes_on_top() {
        let (_, clips, r) = run(&[media("v1", 0, 60), media("v1", 100, 60), Op::MoveClip { id: "c1".into(), track: None, start: 120 }]);
        r.unwrap();
        assert_eq!(clips.last().unwrap().id, "c1", "перенесенный — сверху");
    }

    #[test]
    fn ops_json_shape() {
        let op: Op = serde_json::from_str(r#"{"op":"moveClip","id":"c1","track":null,"start":12}"#).unwrap();
        assert_eq!(op, Op::MoveClip { id: "c1".into(), track: None, start: 12 });
    }
}
