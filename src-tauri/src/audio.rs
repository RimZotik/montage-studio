//! Анализ звука (`docs/extensions.md`): решает ядро, а не модель — пики (удары) для нарезки, паузы
//! для вырезания и приглушения музыки, громкость для нормализации. Звук читается через ffmpeg в
//! память; расчеты — чистые функции, они проверяются тестами без файлов.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{AppError, AppResult};
use crate::media::quiet;

/// Частота чтения звука для анализа: удары и паузы различимы, память небольшая.
pub const RATE: u32 = 11_025;

/// Какие частоты слушать.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Full,
    /// Низкие — бочка и бас: нарезка под тяжелую музыку.
    Bass,
}

impl Band {
    pub fn parse(s: &str) -> Self {
        if s == "bass" { Self::Bass } else { Self::Full }
    }
}

/// Звук файла: моно, `RATE` Гц, значения −1…1.
pub fn decode(ffmpeg: &Path, file: &Path, band: Band) -> AppResult<Vec<f32>> {
    let mut c = Command::new(ffmpeg);
    c.args(["-v", "error", "-i"]).arg(file).args(["-vn", "-ac", "1", "-ar", &RATE.to_string()]);
    if band == Band::Bass {
        c.args(["-af", "lowpass=f=150"]);
    }
    c.args(["-f", "s16le", "-"]).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = quiet(&mut c).output().map_err(|e| AppError::new("ffmpeg", e.to_string()))?;
    if !out.status.success() {
        return Err(AppError::new("ffmpeg", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("decode failed").to_owned()));
    }
    Ok(out.stdout.as_chunks::<2>().0.iter().map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0).collect())
}

/// Среднеквадратичная громкость окнами по `hop` отсчетов.
fn rms(samples: &[f32], hop: usize) -> Vec<f32> {
    samples.chunks(hop).map(|w| (w.iter().map(|x| x * x).sum::<f32>() / w.len() as f32).sqrt()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// Секунды от начала файла.
    pub t: f64,
    /// Сила относительно самого сильного удара, 0–1.
    pub strength: f32,
}

/// Удары: резкий рост громкости выше среднего по соседней секунде. `sensitivity` 0–1 (больше —
/// больше ударов), `min_gap` — наименьшее расстояние, секунд (из близких остается сильнейший).
pub fn peaks(samples: &[f32], rate: u32, sensitivity: f64, min_gap: f64) -> Vec<Peak> {
    const HOP: usize = 128;
    const NEAR: usize = 3;
    let log: Vec<f32> = rms(samples, HOP).iter().map(|e| (1.0 + 100.0 * e).ln()).collect();
    let flux: Vec<f32> = std::iter::once(0.0).chain(log.windows(2).map(|w| (w[1] - w[0]).max(0.0))).collect();
    let hop_s = HOP as f64 / f64::from(rate);
    let span = (0.5 / hop_s) as usize;
    let k = 2.5 - 2.0 * sensitivity.clamp(0.0, 1.0) as f32;
    let mut found: Vec<(usize, f32)> = Vec::new();
    for i in 0..flux.len() {
        let w = &flux[i.saturating_sub(span)..(i + span + 1).min(flux.len())];
        let mean = w.iter().sum::<f32>() / w.len() as f32;
        let dev = (w.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / w.len() as f32).sqrt();
        let around = &flux[i.saturating_sub(NEAR)..(i + NEAR + 1).min(flux.len())];
        if flux[i] > 0.01 && flux[i] > mean + k * dev && around.iter().all(|x| *x <= flux[i]) {
            found.push((i, flux[i]));
        }
    }
    // Сначала сильные: слабый рядом с уже взятым отбрасывается.
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    let top = found.first().map_or(1.0, |f| f.1);
    let gap = (min_gap.max(0.0) / hop_s) as usize;
    let mut kept: Vec<(usize, f32)> = Vec::new();
    for f in found {
        if kept.iter().all(|k| k.0.abs_diff(f.0) >= gap.max(1)) {
            kept.push(f);
        }
    }
    kept.sort_by_key(|k| k.0);
    kept.into_iter().map(|(i, s)| Peak { t: i as f64 * hop_s, strength: s / top }).collect()
}

/// Тихие участки длиннее `min_s`, тише `threshold_db` (секунды файла).
pub fn silences(samples: &[f32], rate: u32, threshold_db: f64, min_s: f64) -> Vec<(f64, f64)> {
    let hop = (rate / 100).max(1) as usize;
    let hop_s = hop as f64 / f64::from(rate);
    let limit = 10f64.powf(threshold_db / 20.0) as f32;
    let levels = rms(samples, hop);
    let mut out = Vec::new();
    let mut from = None;
    for (i, level) in levels.iter().enumerate() {
        match (*level < limit, from) {
            (true, None) => from = Some(i),
            (false, Some(a)) => {
                if (i - a) as f64 * hop_s >= min_s {
                    out.push((a as f64 * hop_s, i as f64 * hop_s));
                }
                from = None;
            }
            _ => {}
        }
    }
    if let Some(a) = from
        && (levels.len() - a) as f64 * hop_s >= min_s
    {
        out.push((a as f64 * hop_s, levels.len() as f64 * hop_s));
    }
    out
}

/// Участки со звуком — то, что между паузами (секунды файла).
pub fn sounding(silent: &[(f64, f64)], total_s: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut at = 0.0;
    for (a, b) in silent {
        if *a > at {
            out.push((at, *a));
        }
        at = *b;
    }
    if total_s > at {
        out.push((at, total_s));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Интегральная громкость, LUFS.
    pub lufs: f64,
    /// Истинный пик, дБ.
    pub peak_db: f64,
    /// Разброс громкости, LU.
    pub range: f64,
}

/// Итог измерения ffmpeg `loudnorm`: последний блок JSON в журнале.
pub fn parse_loudness(log: &str) -> Option<Loudness> {
    let from = log.rfind('{')?;
    let to = log.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(log.get(from..=to)?).ok()?;
    let num = |k: &str| v[k].as_str().and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite());
    Some(Loudness { lufs: num("input_i")?, peak_db: num("input_tp")?, range: num("input_lra").unwrap_or(0.0) })
}

pub fn loudness(ffmpeg: &Path, file: &Path) -> AppResult<Loudness> {
    let mut c = Command::new(ffmpeg);
    c.args(["-hide_banner", "-nostats", "-i"]).arg(file).args(["-vn", "-af", "loudnorm=print_format=json", "-f", "null", "-"]);
    let out = quiet(&mut c).output().map_err(|e| AppError::new("ffmpeg", e.to_string()))?;
    parse_loudness(&String::from_utf8_lossy(&out.stderr)).ok_or_else(|| AppError::new("noAudio", file.display().to_string()))
}

/// Множитель громкости, чтобы довести `lufs` до цели.
pub fn gain_to(lufs: f64, target: f64) -> f64 {
    10f64.powf((target - lufs) / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Тишина с короткими щелчками в заданные секунды.
    fn clicks(total_s: f64, at: &[f64]) -> Vec<f32> {
        let mut s = vec![0.001f32; (total_s * f64::from(RATE)) as usize];
        for t in at {
            let i = (t * f64::from(RATE)) as usize;
            for (n, x) in s[i..i + 400].iter_mut().enumerate() {
                *x = if n % 2 == 0 { 0.9 } else { -0.9 };
            }
        }
        s
    }

    #[test]
    fn peaks_find_hits_and_respect_gap() {
        let at = [1.0, 2.0, 3.0, 4.0];
        let found = peaks(&clicks(5.0, &at), RATE, 0.5, 0.25);
        assert_eq!(found.len(), 4, "{found:?}");
        for (p, t) in found.iter().zip(at) {
            assert!((p.t - t).abs() < 0.03, "{p:?} vs {t}");
        }
        assert_eq!(peaks(&clicks(5.0, &at), RATE, 0.5, 1.5).len(), 2, "близкие отброшены");
        assert!(peaks(&vec![0.001; 44_100], RATE, 1.0, 0.1).is_empty(), "в тишине ударов нет");
    }

    #[test]
    fn silences_and_sounding() {
        let mut s = vec![0.5f32; RATE as usize];
        s.extend(vec![0.0f32; RATE as usize]);
        s.extend(vec![0.5f32; RATE as usize]);
        s.extend(vec![0.0f32; (RATE / 10) as usize]);
        s.extend(vec![0.5f32; RATE as usize]);
        let quiet_parts = silences(&s, RATE, -40.0, 0.5);
        assert_eq!(quiet_parts.len(), 1, "короткая пауза не считается: {quiet_parts:?}");
        assert!((quiet_parts[0].0 - 1.0).abs() < 0.02 && (quiet_parts[0].1 - 2.0).abs() < 0.02);
        let loud = sounding(&quiet_parts, 4.1);
        assert_eq!(loud.len(), 2);
        assert!((loud[1].1 - 4.1).abs() < 1e-9);
        assert_eq!(silences(&vec![0.0; RATE as usize], RATE, -40.0, 0.5).len(), 1, "тишина до конца файла");
    }

    #[test]
    fn loudness_log_and_gain() {
        let log = "size=N/A\n[Parsed_loudnorm_0 @ 0x1] \n{\n\t\"input_i\" : \"-20.00\",\n\t\"input_tp\" : \"-3.10\",\n\t\"input_lra\" : \"5.20\"\n}\n";
        assert_eq!(parse_loudness(log), Some(Loudness { lufs: -20.0, peak_db: -3.1, range: 5.2 }));
        assert_eq!(parse_loudness("{\"input_i\":\"-inf\",\"input_tp\":\"-inf\"}"), None, "тишина не измеряется");
        assert!((gain_to(-20.0, -14.0) - 1.995).abs() < 0.01);
    }
}
