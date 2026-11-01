import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Track } from "@/shared/ipc/bindings";

/** Текст элемента библиотеки: строка или `{ru, en}`. */
export function loc(v: unknown, lang: string): string {
  if (typeof v === "string") return v;
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    const s = o[lang] ?? o.ru ?? o.en;
    if (typeof s === "string") return s;
  }
  return "";
}

const UNITS_RU: Record<string, string> = { s: "с", ms: "мс", Hz: "Гц", dB: "дБ", px: "px" };

/** Единица измерения на языке интерфейса. */
function unitOf(u: unknown, lang: string): string {
  const text = loc(u, lang);
  return lang === "ru" ? (UNITS_RU[text] ?? text) : text;
}

/** Значения панели по умолчанию. */
export function defaultsOf(controls: unknown): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const c of Array.isArray(controls) ? controls : []) if (c?.id) out[c.id] = c.default ?? null;
  return out;
}

/** Ползунок применяет значение, когда его отпустили: одно движение — один шаг отмены. */
export function Slider({ label, help, value, min, max, step, unit, scale = 1, onCommit }: { label: string; help?: string; value: number; min: number; max: number; step: number; unit: string; scale?: number; onCommit: (v: number) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <label className="sl" title={help}>
      <span className="k">{label}</span>
      <input type="range" min={min} max={max} step={step} value={v} onChange={(e) => setV(Number(e.target.value))} onPointerUp={() => v !== value && onCommit(v)} onKeyUp={() => v !== value && onCommit(v)} />
      <span className="v mono">
        {Math.round(v * scale * 100) / 100}
        {unit}
      </span>
    </label>
  );
}

function Hex({ value, onCommit }: { value: string; onCommit: (v: string) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  const solid = /^#[0-9a-f]{6}/i.test(v) ? v.slice(0, 7) : "#000000";
  return (
    <span className="colorctl">
      <input type="color" value={solid} onChange={(e) => setV(e.target.value)} onBlur={() => v !== value && onCommit(v)} />
      <input className="field mono" value={v} onChange={(e) => setV(e.target.value)} onBlur={() => v !== value && onCommit(v)} />
    </span>
  );
}

function Field({ value, type, min, max, step, onCommit }: { value: string; type: string; min?: number; max?: number; step?: number; onCommit: (v: string) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return <input className="field" type={type} min={min} max={max} step={step} value={v} onChange={(e) => setV(e.target.value)} onBlur={() => v !== value && onCommit(v)} onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()} />;
}

/** Панель управления элемента библиотеки: строится из данных (`controls`), ее создает и меняет
 *  Claude или пользователь, в коде ничего не зафиксировано. Незнакомый тип — простое поле. */
export function ControlPanel({ controls, values, tracks, onChange }: { controls: unknown; values: Record<string, unknown>; tracks?: Track[]; onChange: (id: string, value: unknown) => void }) {
  const { i18n } = useTranslation();
  const lang = i18n.language;
  const list: any[] = Array.isArray(controls) ? controls : [];
  const val = (c: any) => values[c.id] ?? c.default;
  return (
    <div className="ctlpanel">
      {list.map((c, i) => {
        if (c.showIf && values[c.showIf.id] !== c.showIf.eq) return null;
        const label = loc(c.label, lang) || c.id;
        const help = loc(c.help, lang) || undefined;
        const key = c.id ?? `d${i}`;
        const wide = c.half ? "half" : "";
        switch (c.type) {
          case "section":
            return (
              <div key={key} className="ctlsec">
                {loc(c.label, lang)}
              </div>
            );
          case "note":
            return (
              <p key={key} className="faint ctlnote">
                {loc(c.text, lang)}
              </p>
            );
          case "slider":
            return <Slider key={key} label={label} help={help} value={Number(val(c) ?? c.min)} min={c.min} max={c.max} step={c.step ?? (c.max - c.min) / 100} unit={c.unit ? ` ${unitOf(c.unit, lang)}` : ""} onCommit={(v) => onChange(c.id, v)} />;
          case "toggle":
            return (
              <label key={key} className={`ctl check ${wide}`} title={help}>
                <input type="checkbox" checked={!!val(c)} onChange={(e) => onChange(c.id, e.target.checked)} />
                <span>{label}</span>
              </label>
            );
          case "select":
            return (
              <label key={key} className={`ctl ${wide}`} title={help}>
                <span className="k">{label}</span>
                <select className="field" value={String(val(c) ?? "")} onChange={(e) => onChange(c.id, e.target.value)}>
                  {(c.options ?? []).map((o: any) => (
                    <option key={o.value} value={o.value}>
                      {loc(o.label, lang) || o.value}
                    </option>
                  ))}
                </select>
              </label>
            );
          case "track":
            return (
              <label key={key} className={`ctl ${wide}`} title={help}>
                <span className="k">{label}</span>
                <select className="field" value={String(val(c) ?? "")} onChange={(e) => onChange(c.id, e.target.value)}>
                  {(tracks ?? []).filter((t) => !c.kinds || c.kinds.includes(t.kind)).map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}
                    </option>
                  ))}
                </select>
              </label>
            );
          case "color":
            return (
              <label key={key} className={`ctl ${wide}`} title={help}>
                <span className="k">{label}</span>
                <Hex value={String(val(c) ?? "#000000")} onCommit={(v) => onChange(c.id, v)} />
              </label>
            );
          case "number":
            return (
              <label key={key} className={`ctl ${wide}`} title={help}>
                <span className="k">
                  {label}
                  {c.unit ? `, ${unitOf(c.unit, lang)}` : ""}
                </span>
                <Field type="number" value={String(val(c) ?? 0)} min={c.min} max={c.max} step={c.step} onCommit={(v) => onChange(c.id, Number(v))} />
              </label>
            );
          default:
            return (
              <label key={key} className={`ctl ${wide}`} title={help}>
                <span className="k">{label}</span>
                <Field type="text" value={typeof val(c) === "string" ? (val(c) as string) : JSON.stringify(val(c) ?? "")} onCommit={(v) => onChange(c.id, v)} />
              </label>
            );
        }
      })}
    </div>
  );
}
