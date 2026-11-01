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
    /// Громкость всей дорожки 0–2.
    #[serde(default = "one")]
    pub volume: f64,
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
    /// Скорость воспроизведения файла: 0.5 — вдвое медленнее.
    #[serde(default = "one")]
    pub speed: f64,
    /// Огибающая громкости: пары «кадр файла, множитель» по возрастанию; между ними — плавно.
    /// Кадры файла, а не клипа: разрез и перенос ее не сбивают.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volume_keys: Vec<(u32, f64)>,
}

/// Изменить часть свойств: незаданные остаются как были.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ClipPropsPatch {
    pub volume: Option<f64>,
    pub opacity: Option<f64>,
    pub scale: Option<f64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub fade_in: Option<u32>,
    pub fade_out: Option<u32>,
    pub speed: Option<f64>,
    pub volume_keys: Option<Vec<(u32, f64)>>,
}

impl ClipProps {
    fn merge(&mut self, p: &ClipPropsPatch) {
        self.volume = p.volume.unwrap_or(self.volume);
        self.opacity = p.opacity.unwrap_or(self.opacity);
        self.scale = p.scale.unwrap_or(self.scale);
        self.x = p.x.unwrap_or(self.x);
        self.y = p.y.unwrap_or(self.y);
        self.fade_in = p.fade_in.unwrap_or(self.fade_in);
        self.fade_out = p.fade_out.unwrap_or(self.fade_out);
        self.speed = p.speed.unwrap_or(self.speed);
        if let Some(k) = &p.volume_keys {
            self.volume_keys.clone_from(k);
        }
    }
}

/// Эффект на клипе: копия из библиотеки вместе со значениями и панелью управления. Проект не
/// зависит от библиотеки, а панель и анимацию можно поменять только у этого клипа.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppliedEffect {
    /// ID на клипе (`fx1`); присваивает ядро.
    #[serde(default)]
    pub id: String,
    /// ID в библиотеке.
    pub effect: String,
    #[specta(type = specta_typescript::Any)]
    pub name: serde_json::Value,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub params: serde_json::Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub controls: serde_json::Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub spec: serde_json::Value,
}

fn yes() -> bool {
    true
}

/// Анимации от края клипа не повторяются на стыке после разреза.
fn drop_anchor(effects: &mut [AppliedEffect], anchor: &str) {
    for fx in effects {
        if let Some(tracks) = fx.spec.get_mut("tracks").and_then(serde_json::Value::as_array_mut) {
            tracks.retain(|t| t["anchor"] != anchor);
        }
    }
}

fn one() -> f64 {
    1.0
}

impl Default for ClipProps {
    fn default() -> Self {
        Self { volume: 1.0, opacity: 1.0, scale: 1.0, x: 0.0, y: 0.0, fade_in: 0, fade_out: 0, speed: 1.0, volume_keys: vec![] }
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
    /// Эффекты по порядку наложения.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<AppliedEffect>,
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
    /// Изменить свойства: незаданные остаются как были.
    SetProps { id: String, props: ClipPropsPatch },
    SetText { id: String, text: String },
    SetTrack {
        id: String,
        #[serde(default)]
        muted: Option<bool>,
        #[serde(default)]
        locked: Option<bool>,
        #[serde(default)]
        volume: Option<f64>,
    },
    /// Наложить эффект на клип.
    AddEffect { clip: String, effect: AppliedEffect },
    /// Изменить эффект на клипе: значения (добавляются к прежним), вкл/выкл, панель, анимацию.
    SetEffect {
        clip: String,
        fx: String,
        #[serde(default)]
        #[specta(type = specta_typescript::Any)]
        params: Option<serde_json::Value>,
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        #[specta(type = specta_typescript::Any)]
        controls: Option<serde_json::Value>,
        #[serde(default)]
        #[specta(type = specta_typescript::Any)]
        spec: Option<serde_json::Value>,
    },
    RemoveEffect { clip: String, fx: String },
    AddMarker { frame: u32, label: String },
    RemoveMarker { frame: u32 },
}

/// Стандартные дорожки нового проекта: две видео, субтитры, голос, музыка, звуки.
pub fn default_tracks() -> Vec<Track> {
    let t = |id: &str, kind, name: &str| Track { id: id.into(), kind, name: name.into(), muted: false, locked: false, volume: 1.0 };
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

    /// Файл только со звуком на видеодорожке — черный кадр и тишина; картинка на звуковой — тишина.
    /// Проверяется при правке, а не при чтении: старый проект с таким клипом все равно открывается.
    fn file_fits(&self, track: &str, src: Option<&str>) -> AppResult<()> {
        let kind = self.track(track)?.kind;
        let category = src.and_then(crate::media::category_of);
        if (kind == TrackKind::Video && category == Some("sfx")) || (kind == TrackKind::Audio && category == Some("images")) {
            return Err(bad(format!("{} cannot go on track {track}: use {} track", src.unwrap_or(""), if kind == TrackKind::Video { "an audio" } else { "a video" })));
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
                self.file_fits(track, src.as_deref())?;
                let id = next_id(self.clips);
                self.clips.push(Clip { id: id.clone(), track: track.clone(), kind: *kind, src: src.clone(), text: text.clone(), start: *start, duration: *duration, offset: offset.unwrap_or(0), link: None, props: ClipProps::default(), effects: vec![] });
                Ok(vec![id])
            }
            Op::AddLinked { video_track, audio_track, src, start, duration, offset } => {
                self.unlocked(video_track)?;
                self.unlocked(audio_track)?;
                let v = next_id(self.clips);
                let link = format!("l{}", v.trim_start_matches('c'));
                let base = Clip { id: v.clone(), track: video_track.clone(), kind: ClipKind::Media, src: Some(src.clone()), text: None, start: *start, duration: *duration, offset: offset.unwrap_or(0), link: Some(link), props: ClipProps::default(), effects: vec![] };
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
                    let src = self.clip_mut(id)?.src.clone();
                    self.file_fits(t, src.as_deref())?;
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
                        right.offset = c.offset + (f64::from(left) * c.props.speed).round() as u32;
                    }
                    right.props.fade_in = 0;
                    drop_anchor(&mut right.effects, "start");
                    if c.link.is_some() {
                        right.link = Some(right_link.clone());
                    }
                    let first = self.clip_mut(g)?;
                    first.duration = left;
                    first.props.fade_out = 0;
                    drop_anchor(&mut first.effects, "end");
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
                self.clip_mut(id)?.props.merge(props);
                Ok(vec![])
            }
            Op::SetText { id, text } => {
                self.clip_mut(id)?.text = Some(text.clone());
                Ok(vec![])
            }
            Op::SetTrack { id, muted, locked, volume } => {
                let t = self.tracks.iter_mut().find(|t| &t.id == id).ok_or_else(|| bad(format!("track {id} not found")))?;
                if let Some(m) = muted {
                    t.muted = *m;
                }
                if let Some(l) = locked {
                    t.locked = *l;
                }
                if let Some(v) = volume {
                    t.volume = *v;
                }
                Ok(vec![])
            }
            Op::AddEffect { clip, effect } => {
                let c = self.clip_mut(clip)?;
                let free = (1..).find(|n| !c.effects.iter().any(|f| f.id == format!("fx{n}"))).unwrap_or(1);
                let id = format!("fx{free}");
                c.effects.push(AppliedEffect { id: id.clone(), ..effect.clone() });
                Ok(vec![id])
            }
            Op::SetEffect { clip, fx, params, enabled, controls, spec } => {
                let c = self.clip_mut(clip)?;
                let f = c.effects.iter_mut().find(|f| &f.id == fx).ok_or_else(|| bad(format!("effect {fx} not on clip {clip}")))?;
                if let Some(p) = params.as_ref().and_then(serde_json::Value::as_object) {
                    if !f.params.is_object() {
                        f.params = serde_json::json!({});
                    }
                    if let Some(all) = f.params.as_object_mut() {
                        all.extend(p.iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                }
                if let Some(e) = enabled {
                    f.enabled = *e;
                }
                if let Some(x) = controls {
                    f.controls.clone_from(x);
                }
                if let Some(x) = spec {
                    f.spec.clone_from(x);
                }
                Ok(vec![])
            }
            Op::RemoveEffect { clip, fx } => {
                self.clip_mut(clip)?.effects.retain(|f| &f.id != fx);
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
        if !(0.1..=8.0).contains(&p.speed) {
            return Err(bad(format!("clip {} speed out of range", c.id)));
        }
        if p.volume_keys.iter().any(|k| !(0.0..=2.0).contains(&k.1)) || p.volume_keys.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(bad(format!("clip {} volume keys", c.id)));
        }
        for fx in &c.effects {
            crate::library::validate_controls(&if fx.controls.is_null() { serde_json::json!([]) } else { fx.controls.clone() })?;
            crate::library::validate_spec(&fx.spec, &fx.controls)?;
        }
    }
    if tracks.iter().any(|t| !(0.0..=2.0).contains(&t.volume)) {
        return Err(bad("track volume out of range"));
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
        let sound = |track: &str, src: &str| Op::AddClip { track: track.into(), kind: ClipKind::Media, src: Some(src.into()), text: None, start: 0, duration: 10, offset: None };
        assert!(run(&[sound("v1", "media/sfx/a.wav")]).2.is_err(), "звук на видеодорожке");
        assert!(run(&[sound("a1", "media/images/a.png")]).2.is_err(), "картинка на звуковой");
        assert!(run(&[sound("a1", "media/footage/a.mp4")]).2.is_ok(), "звук видео на звуковой — можно");
    }

    #[test]
    fn locked_track_and_bad_split() {
        assert_eq!(run(&[Op::SetTrack { id: "v1".into(), muted: None, locked: Some(true), volume: None }, media("v1", 0, 10)]).2.unwrap_err().code, "trackLocked");
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
    fn split_respects_speed() {
        let (_, clips, r) = run(&[media("v1", 0, 100), Op::SetProps { id: "c1".into(), props: ClipPropsPatch { speed: Some(0.5), ..Default::default() } }, Op::SplitClip { id: "c1".into(), at: 40 }]);
        r.unwrap();
        assert_eq!(clips.iter().find(|c| c.id != "c1").unwrap().offset, 20, "40 кадров таймлайна — 20 кадров файла");
    }

    #[test]
    fn partial_props_keep_the_rest() {
        let keys = vec![(10, 0.5), (20, 1.0)];
        let (_, clips, r) = run(&[media("a1", 0, 60), Op::SetProps { id: "c1".into(), props: ClipPropsPatch { scale: Some(2.0), volume_keys: Some(keys.clone()), ..Default::default() } }, Op::SetProps { id: "c1".into(), props: ClipPropsPatch { volume: Some(0.5), ..Default::default() } }]);
        r.unwrap();
        let p = &clips[0].props;
        assert_eq!((p.volume, p.scale, p.volume_keys.clone()), (0.5, 2.0, keys));
        let (_, _, bad_keys) = run(&[media("a1", 0, 60), Op::SetProps { id: "c1".into(), props: ClipPropsPatch { volume_keys: Some(vec![(20, 1.0), (10, 0.5)]), ..Default::default() } }]);
        assert!(bad_keys.is_err(), "ключи по возрастанию");
    }

    #[test]
    fn effects_on_clip_and_split() {
        let spec = serde_json::json!({ "tracks": [{ "prop": "opacity", "anchor": "start", "keys": [[0, 0], ["$d", 1]] }, { "prop": "opacity", "anchor": "end", "keys": [[-1, 1], [0, 0]] }, { "prop": "blur", "value": "$d" }] });
        let controls = serde_json::json!([{ "type": "slider", "id": "d", "min": 0, "max": 5, "default": 1 }]);
        let fx = AppliedEffect { id: String::new(), effect: "t".into(), name: serde_json::json!("Т"), enabled: true, params: serde_json::json!({ "d": 1 }), controls, spec };
        let (_, clips, r) = run(&[
            media("v1", 0, 60),
            Op::AddEffect { clip: "c1".into(), effect: fx.clone() },
            Op::AddEffect { clip: "c1".into(), effect: fx.clone() },
            Op::SetEffect { clip: "c1".into(), fx: "fx2".into(), params: Some(serde_json::json!({ "d": 3 })), enabled: Some(false), controls: None, spec: None },
            Op::RemoveEffect { clip: "c1".into(), fx: "fx1".into() },
            Op::SplitClip { id: "c1".into(), at: 30 },
        ]);
        r.unwrap();
        let left = clips.iter().find(|c| c.id == "c1").unwrap();
        let right = clips.iter().find(|c| c.id != "c1").unwrap();
        assert_eq!((left.effects[0].id.as_str(), left.effects[0].params["d"].as_i64(), left.effects[0].enabled), ("fx2", Some(3), false));
        let anchors = |c: &Clip| c.effects[0].spec["tracks"].as_array().unwrap().iter().map(|t| t["anchor"].as_str().unwrap_or("none").to_owned()).collect::<Vec<_>>();
        assert_eq!(anchors(left), ["start", "none"], "конец у левой части не играет");
        assert_eq!(anchors(right), ["end", "none"], "начало у правой части не играет");
        let (_, _, refused) = run(&[media("v1", 0, 60), Op::AddEffect { clip: "c1".into(), effect: AppliedEffect { controls: serde_json::json!([]), ..fx } }]);
        assert!(refused.is_err(), "ссылка на несуществующий элемент панели");
    }

    #[test]
    fn ops_json_shape() {
        let op: Op = serde_json::from_str(r#"{"op":"moveClip","id":"c1","track":null,"start":12}"#).unwrap();
        assert_eq!(op, Op::MoveClip { id: "c1".into(), track: None, start: 12 });
    }
}
