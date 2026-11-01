import type { AppliedEffect } from "@/shared/ipc/bindings";

// Эффекты клипа (docs/extensions.md): описание-данные превращается в значения для кадра. Чистые
// функции: один и тот же кадр дает одно и то же в предпросмотре и при рендере.

type Vars = Record<string, unknown>;

/** Число или выражение над значениями панели: `$a`, `-$a`, `$a*2+1` (слева направо, без скобок). */
export function evalExpr(v: unknown, vars: Vars): number {
  if (typeof v === "number") return v;
  if (typeof v !== "string") return 0;
  let total = 0;
  let op = "+";
  const tokens = v.trim().match(/[+\-*/]|[^+\-*/\s]+/g) ?? [];
  // Минус первым — знак числа, не операция.
  for (const t of tokens) {
    if ("+-*/".includes(t)) {
      op = op !== "+" && t === "-" ? op : t;
      continue;
    }
    const raw = t.startsWith("$") ? Number(vars[t.slice(1)]) : Number(t);
    const n = Number.isFinite(raw) ? raw : 0;
    total = op === "+" ? total + n : op === "-" ? total - n : op === "*" ? total * n : n === 0 ? total : total / n;
    op = "+";
  }
  return total;
}

const EASE: Record<string, (t: number) => number> = {
  linear: (t) => t,
  in: (t) => t * t,
  out: (t) => 1 - (1 - t) * (1 - t),
  inOut: (t) => (t < 0.5 ? 2 * t * t : 1 - 2 * (1 - t) * (1 - t)),
  back: (t) => 1 + 2.7 * Math.pow(t - 1, 3) + 1.7 * Math.pow(t - 1, 2),
};

/** Значение по ключам `[время, значение]`: до первого и после последнего — крайние. */
function atKeys(keys: [number, number][], t: number, ease: (x: number) => number): number {
  const first = keys[0];
  const last = keys[keys.length - 1];
  if (!first || !last) return 0;
  if (t <= first[0]) return first[1];
  if (t >= last[0]) return last[1];
  for (let i = 1; i < keys.length; i++) {
    const a = keys[i - 1] as [number, number];
    const b = keys[i] as [number, number];
    if (t <= b[0]) return b[0] === a[0] ? b[1] : a[1] + (b[1] - a[1]) * ease((t - a[0]) / (b[0] - a[0]));
  }
  return last[1];
}

export type FxState = { opacity: number; scale: number; x: number; y: number; rotate: number; volume: number; filter: string; color?: string; background?: string };

const MULTIPLY = new Set(["opacity", "scale", "volume", "brightness", "contrast", "saturate"]);

/** Итог всех включенных эффектов клипа в кадре `frame` (от начала клипа). */
export function effectsAt(effects: AppliedEffect[] | undefined, frame: number, duration: number, fps: number): FxState {
  const v: Record<string, number> = { opacity: 1, scale: 1, x: 0, y: 0, rotate: 0, volume: 1, blur: 0, brightness: 1, contrast: 1, saturate: 1, grayscale: 0, sepia: 0, hue: 0 };
  const out: FxState = { opacity: 1, scale: 1, x: 0, y: 0, rotate: 0, volume: 1, filter: "" };
  for (const fx of effects ?? []) {
    if (fx.enabled === false) continue;
    const vars: Vars = { ...fx.params, clip: duration / fps, fps };
    for (const tr of fx.spec?.tracks ?? []) {
      const prop = String(tr.prop);
      const anchor = tr.anchor ?? "none";
      const t = anchor === "end" ? (frame - duration) / fps : anchor === "span" ? frame / Math.max(1, duration) : frame / fps;
      let value: number;
      if (tr.wave) value = (MULTIPLY.has(prop) ? 1 : 0) + evalExpr(tr.wave.amp, vars) * Math.sin(2 * Math.PI * evalExpr(tr.wave.freq, vars) * (frame / fps));
      else if (Array.isArray(tr.keys) && tr.keys.length) value = atKeys(tr.keys.map((k: unknown[]) => [evalExpr(k[0], vars), evalExpr(k[1], vars)] as [number, number]), t, EASE[tr.ease as string] ?? EASE.linear ?? ((x) => x));
      else value = evalExpr(tr.value, vars);
      if (MULTIPLY.has(prop)) v[prop] = (v[prop] ?? 1) * value;
      else v[prop] = (v[prop] ?? 0) + value;
    }
    const text = fx.spec?.text;
    if (text?.color) out.color = String(typeof text.color === "string" && text.color.startsWith("$") ? vars[text.color.slice(1)] : text.color);
    if (text?.background) out.background = String(typeof text.background === "string" && text.background.startsWith("$") ? vars[text.background.slice(1)] : text.background);
  }
  const f: string[] = [];
  if (v.blur) f.push(`blur(${v.blur}px)`);
  if (v.brightness !== 1) f.push(`brightness(${v.brightness})`);
  if (v.contrast !== 1) f.push(`contrast(${v.contrast})`);
  if (v.saturate !== 1) f.push(`saturate(${v.saturate})`);
  if (v.grayscale) f.push(`grayscale(${v.grayscale})`);
  if (v.sepia) f.push(`sepia(${v.sepia})`);
  if (v.hue) f.push(`hue-rotate(${v.hue}deg)`);
  return { ...out, opacity: Math.max(0, Math.min(1, v.opacity ?? 1)), scale: v.scale ?? 1, x: v.x ?? 0, y: v.y ?? 0, rotate: v.rotate ?? 0, volume: Math.max(0, v.volume ?? 1), filter: f.join(" ") };
}

/** Множитель огибающей громкости в кадре файла. */
export function volumeAt(keys: [number, number][] | undefined, sourceFrame: number): number {
  return keys && keys.length ? atKeys(keys, sourceFrame, (t) => t) : 1;
}
