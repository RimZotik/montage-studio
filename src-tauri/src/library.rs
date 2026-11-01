//! Библиотека эффектов и инструментов (`docs/extensions.md`): папки-паки в
//! `Документы\Montage Studio\Library\packs\`. `main` — главный пак (все без своего пака), `base` —
//! базовый пак программы (только чтение, обновляется с программой), остальные — паки пользователя.
//! Элемент живет в одном паке, другие ссылаются на него по ID; архив пака содержит копии всего.
//!
//! Эффект — данные (анимация свойств, `spec`), инструмент — данные (встроенный алгоритм или
//! шаблон операций таймлайна). Панель управления тоже данные (`controls`): ее создает и меняет
//! Claude или пользователь, ползунки ни в коде, ни в формате не зафиксированы. Кода в элементах пока
//! нет, поэтому чужой архив безопасен.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::project::timeline::AppliedEffect;
use crate::project::write_atomic;

pub const MAIN: &str = "main";
pub const BASE: &str = "base";
const BASE_PACK: &str = include_str!("../resources/library/base-pack.json");
/// Встроенные алгоритмы инструментов (`presets.rs`).
pub const BUILTINS: [&str; 9] = ["beatCut", "removeSilence", "captionsFromSpeech", "normalize", "duck", "audioProcess", "cutSounds", "loopEnd", "blurBackground"];
/// Фильтры и источники звука, которые можно назвать в данных элемента: только обработка, ни один
/// не читает и не пишет файлы, поэтому чужой архив безопасен.
const AUDIO_FILTERS: [&str; 30] = ["aevalsrc", "anoisesrc", "sine", "highpass", "lowpass", "bandpass", "bandreject", "equalizer", "bass", "treble", "afftdn", "agate", "acompressor", "alimiter", "loudnorm", "dynaudnorm", "deesser", "aecho", "chorus", "flanger", "aphaser", "tremolo", "vibrato", "atempo", "asetrate", "aresample", "volume", "afade", "stereotools", "crystalizer"];
const PROPS: [&str; 13] = ["opacity", "scale", "x", "y", "rotate", "blur", "brightness", "contrast", "saturate", "grayscale", "sepia", "hue", "volume"];
const ANCHORS: [&str; 4] = ["start", "end", "span", "none"];
/// Элементы панели без значения.
const DECOR: [&str; 2] = ["section", "note"];
const MAX_ARCHIVE_ENTRY: u64 = 1 << 20;

fn array() -> Value {
    json!([])
}

fn one() -> u32 {
    1
}

fn user() -> String {
    "user".into()
}

/// Эффект или инструмент — содержимое `effect.json` / `tool.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(default = "one")]
    pub version: u32,
    pub id: String,
    /// `effect`, `tool`, `style` (правила оформления ролика) или `sound` (звук, синтезируется).
    pub kind: String,
    /// Строка или `{ru, en}`.
    #[specta(type = specta_typescript::Any)]
    pub name: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub description: Value,
    /// Назначение: `video`, `audio`, `text`, `sequence`, `transition`.
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Панель управления: элементы по порядку (`docs/extensions.md`).
    #[serde(default = "array")]
    #[specta(type = specta_typescript::Any)]
    pub controls: Value,
    /// Эффект: что и как анимируется.
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub spec: Value,
    /// Инструмент: `builtin` или `ops`.
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub builtin: Option<String>,
    /// Инструмент `ops`: шаблон операций таймлайна.
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub steps: Value,
    /// Инструмент `ops`: `selection` — шаблон на каждый выбранный клип, иначе один раз.
    #[serde(default)]
    pub each: Option<String>,
    /// Инструмент `audioProcess`: цепочка обработки звука; значения панели — `$id`.
    #[serde(default)]
    pub filter: Option<String>,
    /// Стиль: текст правил (markdown), строка или `{ru, en}`.
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub rules: Value,
    /// Звук: как его синтезировать.
    #[serde(default)]
    pub synth: Option<String>,
    /// Звук: длительность и где слышен удар, секунд.
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub hit_at: Option<f64>,
    /// Предпросмотр на карточке: `{type:"wave"|"blocks"|"image", before, after}` — данные, файлов нет.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[specta(type = specta_typescript::Any)]
    pub preview: Value,
    /// `studio`, `user`, `claude`.
    #[serde(default = "user")]
    pub author: String,
    /// Номер правки элемента.
    #[serde(default = "one")]
    pub revision: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    #[serde(default = "one")]
    pub version: u32,
    pub id: String,
    #[specta(type = specta_typescript::Any)]
    pub name: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Any)]
    pub description: Value,
    #[serde(default = "user")]
    pub author: String,
    /// Базовый пак: менять нельзя, только копировать.
    #[serde(default)]
    pub readonly: bool,
    /// Элементы других паков, добавленные сюда.
    #[serde(default)]
    pub refs: Vec<String>,
    /// Версия содержимого базового пака.
    #[serde(default)]
    pub pack_version: u32,
    /// Порядок элементов, заданный пользователем; не упомянутые идут после, добавленные — в конец.
    #[serde(default)]
    pub order: Vec<String>,
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::new("invalidLibrary", msg)
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !id.starts_with('-')
}

/// Имена `$x` в выражении.
fn refs_of(expr: &str) -> Vec<String> {
    expr.split('$').skip(1).map(|r| r.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()).collect()
}

/// Панель управления: типы не ограничены списком (незнакомый интерфейс покажет простым полем),
/// проверяются только ID и диапазоны.
pub fn validate_controls(controls: &Value) -> AppResult<()> {
    let list = controls.as_array().ok_or_else(|| bad("controls must be an array"))?;
    let mut seen: Vec<&str> = Vec::new();
    for c in list {
        let kind = c["type"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| bad("control without type"))?;
        if DECOR.contains(&kind) {
            continue;
        }
        let id = c["id"].as_str().filter(|i| !i.is_empty()).ok_or_else(|| bad(format!("{kind} control without id")))?;
        if seen.contains(&id) {
            return Err(bad(format!("duplicate control {id}")));
        }
        seen.push(id);
        if kind == "slider" {
            let (min, max) = (c["min"].as_f64(), c["max"].as_f64());
            if !min.zip(max).is_some_and(|(a, b)| a < b) {
                return Err(bad(format!("slider {id}: min must be below max")));
            }
        }
        if kind == "select" && !c["options"].as_array().is_some_and(|o| !o.is_empty()) {
            return Err(bad(format!("select {id} without options")));
        }
    }
    Ok(())
}

fn number_or_ref(v: &Value, known: &[&str], at: &str) -> AppResult<()> {
    match v {
        Value::Number(_) => Ok(()),
        Value::String(s) => {
            for r in refs_of(s) {
                if !known.contains(&r.as_str()) {
                    return Err(bad(format!("{at}: unknown value ${r}")));
                }
            }
            Ok(())
        }
        _ => Err(bad(format!("{at}: number or expression expected"))),
    }
}

/// Описание анимации: известные свойства, ссылки только на элементы панели.
pub fn validate_spec(spec: &Value, controls: &Value) -> AppResult<()> {
    if spec.is_null() {
        return Ok(());
    }
    let ids: Vec<&str> = controls.as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).chain(["clip", "fps"]).collect();
    for t in spec["tracks"].as_array().into_iter().flatten() {
        let prop = t["prop"].as_str().unwrap_or("");
        if !PROPS.contains(&prop) {
            return Err(bad(format!("unknown prop {prop}")));
        }
        let anchor = t["anchor"].as_str().unwrap_or("none");
        if !ANCHORS.contains(&anchor) {
            return Err(bad(format!("unknown anchor {anchor}")));
        }
        for k in t["keys"].as_array().into_iter().flatten() {
            let pair = k.as_array().filter(|p| p.len() == 2).ok_or_else(|| bad(format!("{prop}: key must be [time, value]")))?;
            number_or_ref(&pair[0], &ids, prop)?;
            number_or_ref(&pair[1], &ids, prop)?;
        }
        if !t["value"].is_null() {
            number_or_ref(&t["value"], &ids, prop)?;
        }
        if !t["wave"].is_null() {
            number_or_ref(&t["wave"]["amp"], &ids, prop)?;
            number_or_ref(&t["wave"]["freq"], &ids, prop)?;
        }
    }
    Ok(())
}

/// Предпросмотр: только данные. Картинка — небольшая встроенная, адреса и файлы не принимаются.
pub fn validate_preview(p: &Value) -> AppResult<()> {
    if p.is_null() {
        return Ok(());
    }
    let side = |v: &Value, numbers: bool| -> bool {
        if numbers {
            v.as_array().is_some_and(|a| a.len() <= 512 && a.iter().all(|n| n.as_f64().is_some_and(|x| (0.0..=1000.0).contains(&x))))
        } else {
            v.as_str().is_some_and(|s| s.len() <= 300_000 && ["data:image/jpeg;base64,", "data:image/png;base64,", "data:image/webp;base64,"].iter().any(|h| s.starts_with(h)))
        }
    };
    let ok = match p["type"].as_str() {
        Some("wave" | "blocks") => side(&p["after"], true) && (p["before"].is_null() || side(&p["before"], true)),
        Some("image") => side(&p["after"], false) && (p["before"].is_null() || side(&p["before"], false)),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(bad("preview: {type:wave|blocks, before?:[numbers], after:[numbers]} or {type:image, before?, after} with small data:image"))
    }
}

/// Прикрепить предпросмотр к элементу (база — только чтение: сначала копия).
pub fn set_preview(root: &Path, id: &str, preview: Value) -> AppResult<Item> {
    let (mut item, pack) = find(root, id).ok_or_else(|| bad(format!("{id} not found")))?;
    if pack == "base" {
        return Err(bad("the base pack is read-only: save a copy under a new id first"));
    }
    item.preview = preview;
    save(root, item, Some(&pack))
}

#[tauri::command]
#[specta::specta]
pub fn library_set_preview(app: tauri::AppHandle, id: String, preview: crate::presets::Json) -> AppResult<()> {
    set_preview(&root_of(&app)?, &id, preview.0)?;
    changed(&app);
    Ok(())
}

pub fn validate_item(item: &Item) -> AppResult<()> {
    if !id_ok(&item.id) {
        return Err(bad(format!("id {:?}: lowercase latin, digits, dashes", item.id)));
    }
    if item.name.is_null() {
        return Err(bad("name required"));
    }
    validate_controls(&item.controls)?;
    validate_preview(&item.preview)?;
    match item.kind.as_str() {
        "effect" => {
            validate_spec(&item.spec, &item.controls)?;
            if item.spec["tracks"].as_array().is_none_or(Vec::is_empty) && item.spec["text"].is_null() {
                return Err(bad("effect needs spec.tracks or spec.text"));
            }
        }
        "tool" => match item.engine.as_deref() {
            Some("builtin") if item.builtin.as_deref() == Some("audioProcess") => {
                let chain = item.filter.as_deref().ok_or_else(|| bad("audioProcess needs filter"))?;
                check_chain(chain)?;
                let ids: Vec<&str> = item.controls.as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).collect();
                if let Some(r) = refs_of(chain).iter().find(|r| !ids.contains(&r.as_str())) {
                    return Err(bad(format!("unknown value ${r}")));
                }
            }
            Some("builtin") if item.builtin.as_deref().is_some_and(|b| BUILTINS.contains(&b)) => {}
            Some("ops") if item.steps.as_array().is_some_and(|s| !s.is_empty() && s.iter().all(|o| o["op"].is_string())) => {}
            other => return Err(bad(format!("tool engine {other:?}: builtin with a known name, or ops with steps"))),
        },
        "style" => {
            if item.rules.is_null() || item.rules.as_str().is_some_and(str::is_empty) {
                return Err(bad("style needs rules"));
            }
        }
        "sound" => {
            check_chain(item.synth.as_deref().ok_or_else(|| bad("sound needs synth"))?)?;
            if !item.seconds.is_some_and(|s| s > 0.0 && s <= 60.0) {
                return Err(bad("sound needs seconds"));
            }
        }
        other => return Err(bad(format!("kind {other}: effect, tool, style or sound"))),
    }
    Ok(())
}

fn folder(kind: &str) -> &'static str {
    match kind {
        "tool" => "tools",
        "style" => "styles",
        "sound" => "sounds",
        _ => "effects",
    }
}

fn file_name(kind: &str) -> &'static str {
    match kind {
        "tool" => "tool.json",
        "style" => "style.json",
        "sound" => "sound.json",
        _ => "effect.json",
    }
}

/// Части цепочки по запятым вне кавычек.
fn chain_parts(chain: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    for c in chain.chars() {
        match c {
            '\'' => quoted = !quoted,
            ',' if !quoted => out.push(String::new()),
            _ => {}
        }
        if (c != ',' || quoted)
            && let Some(last) = out.last_mut()
        {
            last.push(c);
        }
    }
    out
}

/// Цепочка звука: только известные фильтры, без подключений к другим потокам и файлам.
pub fn check_chain(chain: &str) -> AppResult<()> {
    if chain.contains(['[', ']', ';']) {
        return Err(bad("audio chain: a plain list of filters"));
    }
    for part in chain_parts(chain) {
        let name = part.split('=').next().unwrap_or("").trim();
        if !AUDIO_FILTERS.contains(&name) {
            return Err(bad(format!("audio chain: filter {name:?} is not allowed")));
        }
    }
    Ok(())
}

/// Подставить значения панели в цепочку: только числа.
pub fn fill_chain(chain: &str, params: &Value) -> AppResult<String> {
    let mut out = chain.to_owned();
    let mut names = refs_of(chain);
    // Сначала длинные: `$a` не должно задеть `$ab`.
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in names {
        let v = params[&n].as_f64().ok_or_else(|| bad(format!("{n} must be a number")))?;
        out = out.replace(&format!("${n}"), &v.to_string());
    }
    Ok(out)
}

fn item_dir(root: &Path, pack: &str, item: &Item) -> PathBuf {
    root.join("packs").join(pack).join(folder(&item.kind)).join(&item.id)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomic(path, &serde_json::to_vec_pretty(value).map_err(|e| AppError::new("internal", e.to_string()))?)
}

fn write_pack(root: &Path, pack: &Pack) -> AppResult<()> {
    write_json(&root.join("packs").join(&pack.id).join("pack.json"), pack)
}

/// Создать главный пак и поставить базовый (при новой версии программы — обновить).
pub fn init(root: &Path) -> AppResult<()> {
    if read_pack(root, MAIN).is_none() {
        write_pack(root, &Pack { version: 1, id: MAIN.into(), name: json!({ "ru": "Главный", "en": "Main" }), description: json!({ "ru": "Все, что не входит в другие паки", "en": "Everything outside other packs" }), author: "user".into(), readonly: false, refs: vec![], pack_version: 0, order: vec![] })?;
    }
    let bundled: Value = serde_json::from_str(BASE_PACK).map_err(|e| AppError::new("internal", e.to_string()))?;
    let pack: Pack = serde_json::from_value(bundled["pack"].clone()).map_err(|e| AppError::new("internal", e.to_string()))?;
    if read_pack(root, BASE).is_some_and(|p| p.pack_version == pack.pack_version) {
        return Ok(());
    }
    let dir = root.join("packs").join(BASE);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    }
    for raw in bundled["items"].as_array().into_iter().flatten() {
        let item: Item = serde_json::from_value(raw.clone()).map_err(|e| AppError::new("internal", format!("base pack: {e}")))?;
        validate_item(&item)?;
        write_json(&item_dir(root, BASE, &item).join(file_name(&item.kind)), &item)?;
    }
    // Последним: прерванная установка повторится при следующем запуске.
    write_pack(root, &pack)?;
    log::info!("base pack {} installed", pack.pack_version);
    Ok(())
}

fn read_pack(root: &Path, id: &str) -> Option<Pack> {
    serde_json::from_slice(&std::fs::read(root.join("packs").join(id).join("pack.json")).ok()?).ok()
}

/// Все паки: главный первым, базовый вторым.
pub fn packs(root: &Path) -> Vec<Pack> {
    let mut all: Vec<Pack> = std::fs::read_dir(root.join("packs")).into_iter().flatten().flatten().filter_map(|e| read_pack(root, &e.file_name().to_string_lossy())).collect();
    let rank = |p: &Pack| match p.id.as_str() {
        MAIN => 0,
        BASE => 1,
        _ => 2,
    };
    all.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.id.cmp(&b.id)));
    all
}

/// Все элементы с паком-хозяином. Поврежденный файл пропускается и не мешает остальным.
pub fn items(root: &Path) -> Vec<(Item, String)> {
    let mut out = Vec::new();
    for pack in packs(root) {
        for kind in ["effect", "tool", "style", "sound"] {
            for entry in std::fs::read_dir(root.join("packs").join(&pack.id).join(folder(kind))).into_iter().flatten().flatten() {
                let path = entry.path().join(file_name(kind));
                match std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Item>(&b).ok()) {
                    Some(item) if validate_item(&item).is_ok() => out.push((item, pack.id.clone())),
                    Some(_) | None if path.exists() => log::warn!("library: skipped {}", path.display()),
                    _ => {}
                }
            }
        }
    }
    out
}

pub fn find(root: &Path, id: &str) -> Option<(Item, String)> {
    items(root).into_iter().find(|(i, _)| i.id == id)
}

/// Создать или обновить элемент. Новый ложится в `pack` (по умолчанию главный); прошлая версия
/// остается в `.history`.
pub fn save(root: &Path, mut item: Item, pack: Option<&str>) -> AppResult<Item> {
    validate_item(&item)?;
    let current = find(root, &item.id);
    let owner = match &current {
        Some((_, p)) => p.clone(),
        None => pack.unwrap_or(MAIN).to_owned(),
    };
    let target = read_pack(root, &owner).ok_or_else(|| bad(format!("pack {owner} not found")))?;
    if target.readonly {
        return Err(AppError::new("packReadonly", format!("{owner}: save a copy with a new id")));
    }
    let dir = item_dir(root, &owner, &item);
    let file = dir.join(file_name(&item.kind));
    if let Some((old, _)) = current {
        if old.kind != item.kind {
            return Err(bad(format!("{} already exists as {}", item.id, old.kind)));
        }
        item.revision = old.revision + 1;
        write_json(&dir.join(".history").join(format!("{}.json", old.revision)), &old)?;
    }
    item.version = 1;
    write_json(&file, &item)?;
    Ok(item)
}

/// Удалить элемент из его пака и из ссылок других паков.
pub fn delete(root: &Path, id: &str) -> AppResult<()> {
    let (item, owner) = find(root, id).ok_or_else(|| bad(format!("{id} not found")))?;
    if read_pack(root, &owner).is_some_and(|p| p.readonly) {
        return Err(AppError::new("packReadonly", owner));
    }
    std::fs::remove_dir_all(item_dir(root, &owner, &item))?;
    for mut p in packs(root) {
        if p.refs.iter().any(|r| r == id) && !p.readonly {
            p.refs.retain(|r| r != id);
            write_pack(root, &p)?;
        }
    }
    Ok(())
}

/// ID пака из названия: латиница как есть, остальное — короткий номер.
fn pack_id_from(root: &Path, name: &str) -> String {
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.trim_matches('-').to_owned();
    let base = if slug.len() >= 2 && slug.len() <= 40 && !slug.contains("--") { slug } else { "pack".to_owned() };
    let mut id = base.clone();
    let mut n = 2;
    while root.join("packs").join(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

pub fn pack_create(root: &Path, name: &str, description: &str) -> AppResult<Pack> {
    if name.trim().is_empty() {
        return Err(bad("pack name required"));
    }
    let pack = Pack { version: 1, id: pack_id_from(root, name), name: json!(name.trim()), description: json!(description), author: "user".into(), readonly: false, refs: vec![], pack_version: 0, order: vec![] };
    write_pack(root, &pack)?;
    Ok(pack)
}

pub fn pack_rename(root: &Path, id: &str, name: &str) -> AppResult<()> {
    let mut p = read_pack(root, id).ok_or_else(|| bad(format!("pack {id} not found")))?;
    if p.readonly {
        return Err(AppError::new("packReadonly", id.to_owned()));
    }
    p.name = json!(name);
    write_pack(root, &p)
}

/// Удалить пак: его собственные элементы переходят в главный, ничего не теряется.
pub fn pack_delete(root: &Path, id: &str) -> AppResult<()> {
    let p = read_pack(root, id).ok_or_else(|| bad(format!("pack {id} not found")))?;
    if p.readonly || id == MAIN {
        return Err(AppError::new("packReadonly", id.to_owned()));
    }
    for (item, owner) in items(root) {
        if owner == id {
            let to = item_dir(root, MAIN, &item);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(item_dir(root, id, &item), to)?;
        }
    }
    std::fs::remove_dir_all(root.join("packs").join(id))?;
    Ok(())
}

/// Добавить элемент в пак ссылкой или убрать его оттуда.
pub fn pack_link(root: &Path, pack: &str, item: &str, add: bool) -> AppResult<()> {
    let mut p = read_pack(root, pack).ok_or_else(|| bad(format!("pack {pack} not found")))?;
    if p.readonly {
        return Err(AppError::new("packReadonly", pack.to_owned()));
    }
    let (_, owner) = find(root, item).ok_or_else(|| bad(format!("{item} not found")))?;
    if add {
        if owner != pack && !p.refs.iter().any(|r| r == item) {
            p.refs.push(item.to_owned());
        }
    } else {
        if owner == pack {
            return Err(bad("the item lives in this pack: delete it instead"));
        }
        p.refs.retain(|r| r != item);
    }
    write_pack(root, &p)
}

/// Задать порядок элементов пака.
pub fn pack_order(root: &Path, pack: &str, ids: Vec<String>) -> AppResult<()> {
    let mut p = read_pack(root, pack).ok_or_else(|| bad(format!("pack {pack} not found")))?;
    if p.readonly {
        return Err(AppError::new("packReadonly", pack.to_owned()));
    }
    p.order = ids;
    write_pack(root, &p)
}

/// Архив пака: `pack.json` и копии всех элементов, включая добавленные из других паков.
pub fn export(root: &Path, pack: &str, to: &Path) -> AppResult<usize> {
    let mut p = read_pack(root, pack).ok_or_else(|| bad(format!("pack {pack} not found")))?;
    let all = items(root);
    let included: Vec<&Item> = all.iter().filter(|(i, owner)| owner == pack || p.refs.contains(&i.id)).map(|(i, _)| i).collect();
    p.refs.clear();
    p.readonly = false;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(to)?);
    let options = zip::write::SimpleFileOptions::default();
    let mut put = |name: String, bytes: Vec<u8>| -> AppResult<()> {
        zip.start_file(name, options).map_err(|e| AppError::new("badArchive", e.to_string()))?;
        zip.write_all(&bytes)?;
        Ok(())
    };
    put("pack.json".into(), pretty(&p))?;
    for item in &included {
        put(format!("{}/{}/{}", folder(&item.kind), item.id, file_name(&item.kind)), pretty(*item))?;
    }
    zip.finish().map_err(|e| AppError::new("badArchive", e.to_string()))?;
    Ok(included.len())
}

fn pretty<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap_or_default()
}

/// Содержимое без номера правки и автора: одинаковые элементы из разных архивов не множатся.
fn same_content(a: &Item, b: &Item) -> bool {
    let strip = |i: &Item| Item { revision: 1, author: String::new(), ..i.clone() };
    strip(a) == strip(b)
}

/// Импорт архива новым паком. Читаются только известные файлы с данными; элемент, который уже
/// есть, не дублируется (становится ссылкой), другой с тем же ID получает новый ID.
pub fn import(root: &Path, from: &Path) -> AppResult<Pack> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(from)?).map_err(|e| AppError::new("badArchive", e.to_string()))?;
    let mut pack: Option<Pack> = None;
    let mut found: Vec<Item> = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| AppError::new("badArchive", e.to_string()))?;
        let name = entry.name().to_owned();
        let known = name == "pack.json" || ["/effect.json", "/tool.json", "/style.json", "/sound.json"].iter().any(|e| name.ends_with(e));
        if !known || entry.size() > MAX_ARCHIVE_ENTRY {
            continue;
        }
        let mut raw = Vec::new();
        entry.take(MAX_ARCHIVE_ENTRY).read_to_end(&mut raw)?;
        if name == "pack.json" {
            pack = serde_json::from_slice(&raw).ok();
        } else {
            let item: Item = serde_json::from_slice(&raw).map_err(|e| AppError::new("badArchive", format!("{name}: {e}")))?;
            validate_item(&item).map_err(|e| AppError::new("badArchive", format!("{name}: {}", e.message)))?;
            found.push(item);
        }
    }
    let source = pack.ok_or_else(|| AppError::new("badArchive", "pack.json missing"))?;
    let label = source.name.get("ru").or_else(|| source.name.get("en")).unwrap_or(&source.name).as_str().unwrap_or("pack").to_owned();
    let mut new_pack = Pack { id: pack_id_from(root, &label), readonly: false, refs: vec![], pack_version: 0, ..source };
    write_pack(root, &new_pack)?;
    let existing = items(root);
    // Занятые ID: библиотека и весь архив, чтобы новый ID не совпал с другим элементом архива.
    let mut taken: Vec<String> = existing.iter().map(|(e, _)| e.id.clone()).chain(found.iter().map(|i| i.id.clone())).collect();
    for mut item in found {
        match existing.iter().find(|(e, _)| e.id == item.id) {
            Some((e, _)) if same_content(e, &item) => {
                new_pack.refs.push(item.id);
                continue;
            }
            Some(_) => {
                let mut n = 2;
                while taken.contains(&format!("{}-{n}", item.id)) {
                    n += 1;
                }
                item.id = format!("{}-{n}", item.id);
                taken.push(item.id.clone());
            }
            None => {}
        }
        write_json(&item_dir(root, &new_pack.id, &item).join(file_name(&item.kind)), &item)?;
    }
    write_pack(root, &new_pack)?;
    Ok(new_pack)
}

/// Значения панели: по умолчанию из `controls`, поверх — заданные.
pub fn with_defaults(controls: &Value, params: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for c in controls.as_array().into_iter().flatten() {
        if let Some(id) = c["id"].as_str() {
            out.insert(id.to_owned(), c.get("default").cloned().unwrap_or(Value::Null));
        }
    }
    for (k, v) in params.as_object().into_iter().flatten() {
        out.insert(k.clone(), v.clone());
    }
    Value::Object(out)
}

/// Копия эффекта для клипа: проект хранит описание целиком и не зависит от библиотеки — правка
/// эффекта или чужой компьютер не меняют уже смонтированное.
pub fn applied(item: &Item, params: &Value) -> AppliedEffect {
    AppliedEffect { id: String::new(), effect: item.id.clone(), name: item.name.clone(), enabled: true, params: with_defaults(&item.controls, params), controls: item.controls.clone(), spec: item.spec.clone() }
}

/// Папка библиотеки.
pub fn root_of<R: tauri::Runtime>(app: &AppHandle<R>) -> AppResult<PathBuf> {
    let docs = app.path().document_dir().map_err(|e| AppError::new("paths", e.to_string()))?;
    Ok(docs.join("Montage Studio").join("Library"))
}

pub fn usage(db: &Db) -> AppResult<HashMap<String, (u32, i64)>> {
    db.with(|c| {
        let mut q = c.prepare("select item, uses, last_used from library_usage")?;
        let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, u32>(1)?, r.get::<_, i64>(2)?))))?;
        Ok(rows.filter_map(Result::ok).collect())
    })
}

pub fn bump(db: &Db, id: &str) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let _ = db.with(|c| {
        c.execute("insert into library_usage (item, uses, last_used) values (?1, 1, ?2) on conflict (item) do update set uses = uses + 1, last_used = ?2", rusqlite::params![id, now])?;
        Ok(())
    });
}

/// Короткий список для контекста Claude: чаще используемое первым; остальное — `library_list`.
pub fn compact_index(root: &Path, used: &HashMap<String, (u32, i64)>, limit: usize) -> String {
    let mut all: Vec<Item> = items(root).into_iter().map(|(i, _)| i).collect();
    all.sort_by_key(|i| (std::cmp::Reverse(used.get(&i.id).map_or(0, |u| u.0)), i.id.clone()));
    let total = all.len();
    let line = |kind: &str| -> String {
        all.iter()
            .take(limit)
            .filter(|i| i.kind == kind)
            .map(|i| {
                let ids: Vec<&str> = i.controls.as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).collect();
                format!("{}({})", i.id, ids.join(","))
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!("[library {total} items; effects: {}; tools: {}; styles: {}; sounds: {}{}]\n", line("effect"), line("tool"), line("style"), line("sound"), if total > limit { "; more: library_list" } else { "" })
}

/// Библиотека для интерфейса.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRow {
    pub item: Item,
    pub pack: String,
    pub uses: u32,
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub root: String,
    pub packs: Vec<Pack>,
    pub items: Vec<LibraryRow>,
}

/// Библиотека поменялась (интерфейс перечитывает).
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
pub struct LibraryChanged;

pub fn changed<R: tauri::Runtime>(app: &AppHandle<R>) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| LibraryChanged.emit(app)));
}

fn ready(app: &AppHandle) -> AppResult<PathBuf> {
    let root = root_of(app)?;
    init(&root)?;
    Ok(root)
}

#[tauri::command]
#[specta::specta]
pub fn library_view(app: AppHandle, db: State<'_, Db>) -> AppResult<LibraryView> {
    let root = ready(&app)?;
    let used = usage(&db)?;
    let rows = items(&root).into_iter().map(|(item, pack)| {
        let u = used.get(&item.id);
        LibraryRow { uses: u.map_or(0, |x| x.0), last_used: u.map(|x| x.1), item, pack }
    });
    Ok(LibraryView { root: root.display().to_string(), packs: packs(&root), items: rows.collect() })
}

#[tauri::command]
#[specta::specta]
pub fn library_save(app: AppHandle, item: Item, pack: Option<String>) -> AppResult<Item> {
    let saved = save(&ready(&app)?, item, pack.as_deref())?;
    changed(&app);
    Ok(saved)
}

#[tauri::command]
#[specta::specta]
pub fn library_delete(app: AppHandle, id: String) -> AppResult<()> {
    delete(&ready(&app)?, &id)?;
    changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn pack_new(app: AppHandle, name: String) -> AppResult<Pack> {
    let p = pack_create(&ready(&app)?, &name, "")?;
    changed(&app);
    Ok(p)
}

#[tauri::command]
#[specta::specta]
pub fn pack_set_name(app: AppHandle, id: String, name: String) -> AppResult<()> {
    pack_rename(&ready(&app)?, &id, &name)?;
    changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn pack_remove(app: AppHandle, id: String) -> AppResult<()> {
    pack_delete(&ready(&app)?, &id)?;
    changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn pack_item(app: AppHandle, pack: String, item: String, add: bool) -> AppResult<()> {
    pack_link(&ready(&app)?, &pack, &item, add)?;
    changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn pack_reorder(app: AppHandle, pack: String, ids: Vec<String>) -> AppResult<()> {
    pack_order(&ready(&app)?, &pack, ids)?;
    changed(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn pack_export(app: AppHandle, id: String, path: String) -> AppResult<u32> {
    Ok(u32::try_from(export(&ready(&app)?, &id, Path::new(&path))?).unwrap_or(0))
}

#[tauri::command]
#[specta::specta]
pub fn pack_import(app: AppHandle, path: String) -> AppResult<Pack> {
    let p = import(&ready(&app)?, Path::new(&path))?;
    changed(&app);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Library");
        init(&root).unwrap();
        (tmp, root)
    }

    fn effect(id: &str, to: f64) -> Item {
        serde_json::from_value(json!({ "id": id, "kind": "effect", "name": "Тест", "targets": ["video"], "controls": [{ "type": "slider", "id": "to", "min": 0, "max": 2, "default": to }], "spec": { "tracks": [{ "prop": "scale", "anchor": "span", "keys": [[0, 1], [1, "$to"]] }] } })).unwrap()
    }

    #[test]
    fn base_pack_installs_and_is_valid() {
        let (_t, root) = lib();
        let p = packs(&root);
        assert_eq!((p[0].id.as_str(), p[1].id.as_str()), (MAIN, BASE), "главный первым, базовый вторым");
        let all = items(&root);
        assert!(all.iter().all(|(_, owner)| owner == BASE));
        for needed in ["remove-silence", "captions-from-speech", "normalize-loudness", "duck-music", "fade-in", "zoom-slow", "voice-clean", "audio-compressor", "blur"] {
            assert!(all.iter().any(|(i, _)| i.id == needed), "{needed}");
        }
        for personal in ["beat-cut", "whoosh-low", "style-short-retention", "loop-end"] {
            assert!(!all.iter().any(|(i, _)| i.id == personal), "личное не идет с программой: {personal}");
        }
        let bundled: Value = serde_json::from_str(BASE_PACK).unwrap();
        assert_eq!(all.len(), bundled["items"].as_array().unwrap().len(), "ни один элемент не отброшен проверкой");
        // Повторный запуск ничего не трогает, новая версия заменяет.
        std::fs::remove_dir_all(root.join("packs/base/effects/fade-in")).unwrap();
        init(&root).unwrap();
        assert!(find(&root, "fade-in").is_none());
    }

    #[test]
    fn base_is_read_only() {
        let (_t, root) = lib();
        let (mut fade, _) = find(&root, "fade-in").unwrap();
        fade.name = json!("Иначе");
        assert_eq!(save(&root, fade.clone(), None).unwrap_err().code, "packReadonly");
        assert_eq!(delete(&root, "fade-in").unwrap_err().code, "packReadonly");
        fade.id = "my-fade".into();
        save(&root, fade, None).unwrap();
        assert_eq!(find(&root, "my-fade").unwrap().1, MAIN, "копия — в главном паке");
    }

    #[test]
    fn revisions_keep_history() {
        let (_t, root) = lib();
        save(&root, effect("zoom2", 1.2), None).unwrap();
        let again = save(&root, effect("zoom2", 1.5), None).unwrap();
        assert_eq!(again.revision, 2);
        assert!(root.join("packs/main/effects/zoom2/.history/1.json").is_file());
    }

    #[test]
    fn bad_items_are_refused() {
        let (_t, root) = lib();
        assert!(save(&root, effect("Bad Id", 1.0), None).is_err());
        let mut i = effect("x", 1.0);
        i.spec["tracks"][0]["keys"][1][1] = json!("$nope");
        assert!(save(&root, i, None).unwrap_err().message.contains("$nope"), "ссылка только на элемент панели");
        let mut i = effect("y", 1.0);
        i.spec["tracks"][0]["prop"] = json!("javascript");
        assert!(save(&root, i, None).is_err());
        let mut i = effect("z", 1.0);
        i.controls = json!([{ "type": "slider", "id": "a", "min": 2, "max": 1 }]);
        i.spec = json!({ "text": { "color": "#fff" } });
        assert!(save(&root, i, None).is_err());
    }

    #[test]
    fn panel_is_free_form() {
        let (_t, root) = lib();
        let mut i = effect("custom", 1.0);
        i.controls = json!([
            { "type": "section", "label": "Группа" },
            { "type": "slider", "id": "to", "min": 0, "max": 5, "default": 2, "unit": "x", "label": { "ru": "Сила", "en": "Power" } },
            { "type": "curve-editor", "id": "future", "default": [0, 1] },
            { "type": "note", "text": "Подсказка" },
        ]);
        assert!(save(&root, i, None).is_ok(), "незнакомый тип элемента не отклоняется");
    }

    #[test]
    fn packs_refs_export_import() {
        let (_t, root) = lib();
        save(&root, effect("own", 1.2), None).unwrap();
        let p = pack_create(&root, "Мой пак", "").unwrap();
        pack_link(&root, &p.id, "own", true).unwrap();
        pack_link(&root, &p.id, "fade-in", true).unwrap();
        assert_eq!(read_pack(&root, &p.id).unwrap().refs.len(), 2, "один элемент сразу в нескольких паках");
        let archive = root.join("my.zip");
        assert_eq!(export(&root, &p.id, &archive).unwrap(), 2);

        // Та же библиотека: одинаковое становится ссылкой, другое с тем же ID — новым.
        save(&root, effect("own", 1.9), None).unwrap();
        let got = import(&root, &archive).unwrap();
        assert_ne!(got.id, p.id);
        assert!(got.refs.contains(&"fade-in".to_owned()));
        assert!(find(&root, "own-2").is_some_and(|(_, owner)| owner == got.id));

        // Удаление пака не теряет его элементы; из ссылок удаленный элемент уходит.
        pack_delete(&root, &got.id).unwrap();
        assert_eq!(find(&root, "own-2").unwrap().1, MAIN);
        delete(&root, "own").unwrap();
        assert!(!read_pack(&root, &p.id).unwrap().refs.contains(&"own".to_owned()));
        assert_eq!(pack_delete(&root, MAIN).unwrap_err().code, "packReadonly");
    }

    #[test]
    fn archive_with_foreign_files_is_read_as_data_only() {
        let (t, root) = lib();
        let path = t.path().join("evil.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        w.start_file("pack.json", o).unwrap();
        w.write_all(json!({ "id": "../../evil", "name": "Evil", "readonly": true }).to_string().as_bytes()).unwrap();
        w.start_file("../../../outside.exe", o).unwrap();
        w.write_all(b"MZ").unwrap();
        w.finish().unwrap();
        let p = import(&root, &path).unwrap();
        assert!(id_ok(&p.id), "ID пака — свой, не из архива: {}", p.id);
        assert!(!p.readonly);
        assert!(!t.path().join("outside.exe").exists());
    }

    #[test]
    fn audio_chains_are_checked() {
        assert!(check_chain("highpass=f=80,acompressor=threshold=-18dB:ratio=3,loudnorm=I=-16").is_ok());
        assert!(check_chain("aevalsrc='0.5*sin(2*PI*90*t)*max(0,1-t)':d=1").is_ok(), "запятая в кавычках — не разделитель");
        assert!(check_chain("amovie=C\\\\secret.wav").is_err(), "фильтр с чтением файла");
        assert!(check_chain("volume=1[a];[a]afifo").is_err());
        assert_eq!(fill_chain("highpass=f=$hp,volume=$hp2", &json!({ "hp": 80, "hp2": 1.5 })).unwrap(), "highpass=f=80,volume=1.5");
        assert!(fill_chain("highpass=f=$hp", &json!({ "hp": "1,amovie=x" })).is_err(), "только числа");
    }

    #[test]
    fn defaults_and_index() {
        let (_t, root) = lib();
        let v = with_defaults(&json!([{ "type": "section" }, { "id": "a", "type": "slider", "default": 1 }, { "id": "b", "type": "toggle", "default": true }]), &json!({ "a": 5 }));
        assert_eq!(v, json!({ "a": 5, "b": true }));
        let used = HashMap::from([("zoom-slow".to_owned(), (9, 0))]);
        let line = compact_index(&root, &used, 100);
        assert!(line.contains("effects: zoom-slow("), "частое — первым: {line}");
        assert!(line.contains("remove-silence("));
        assert!(line.len() < 2400, "короткая строка: {} знаков", line.len());
    }
}
