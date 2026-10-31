import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AppError, ProjectView, Sequence } from "@/shared/ipc/bindings";
import { MediaColumn } from "./MediaColumn";
import { RulesColumn } from "./RulesColumn";

const PRESETS: { key: string; width: number; height: number }[] = [
  { key: "vertical", width: 1080, height: 1920 },
  { key: "horizontal", width: 1920, height: 1080 },
  { key: "square", width: 1080, height: 1080 },
];
const FPS = [24, 25, 30, 50, 60];

/** Число, которое применяется по Enter или при уходе из поля (а не на каждый символ). */
function NumberField({ value, suffix, onCommit }: { value: number; suffix: string; onCommit: (n: number) => void }) {
  const [text, setText] = useState(String(value));
  useEffect(() => setText(String(value)), [value]);
  const commit = () => {
    const n = Number.parseInt(text, 10);
    if (Number.isFinite(n) && n !== value) onCommit(n);
    else setText(String(value));
  };
  return (
    <label className="field numf">
      <input value={text} inputMode="numeric" onChange={(e) => setText(e.target.value.replace(/[^\d]/g, ""))} onBlur={commit} onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()} />
      <span>{suffix}</span>
    </label>
  );
}

type Props = { view: ProjectView; onSequence: (s: Sequence) => void; onRename: (name: string) => void; onError: (e: AppError) => void; onImported: (n: number) => void; rulesKey: number };

/** «Подготовка»: медиа проекта, правила для Claude, имя и последовательность (с отменой). */
export function Prepare({ view, onSequence, onRename, onError, onImported, rulesKey }: Props) {
  const { t } = useTranslation();
  const seq = view.project.sequence;
  const [name, setName] = useState(view.project.name);
  const [bg, setBg] = useState(seq.background ?? "#000000");
  useEffect(() => setName(view.project.name), [view.project.name]);
  useEffect(() => setBg(seq.background ?? "#000000"), [seq.background]);
  const set = (patch: Partial<Sequence>) => onSequence({ ...seq, ...patch });

  return (
    <div className="prepare">
      <MediaColumn projectPath={view.path} onError={onError} onImported={onImported} />
      <RulesColumn onError={onError} reloadKey={rulesKey} />

      <section className="col seq">
        <div className="colhead">{t("prepare.project")}</div>
        <div className="lbl">{t("prepare.name")}</div>
        <input
          className="field"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onBlur={() => (name.trim() && name !== view.project.name ? onRename(name) : setName(view.project.name))}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        />
        <div className="lbl">{t("prepare.folder")}</div>
        <div className="pathline" title={view.path}>
          {view.path}
        </div>

        <div className="colhead sub">{t("prepare.sequence")}</div>
        <div className="lbl">{t("prepare.size")}</div>
        <div className="presets compact">
          {PRESETS.map((p) => (
            <button key={p.key} className={`preset ${seq.width === p.width && seq.height === p.height ? "on" : ""}`} onClick={() => set({ width: p.width, height: p.height })}>
              <span className="shape" style={{ width: p.width > p.height ? 20 : p.width === p.height ? 15 : 11, height: p.height > p.width ? 20 : p.width === p.height ? 15 : 11 }} />
              {t(`newProject.${p.key}`)}
            </button>
          ))}
        </div>
        <div className="pair">
          <NumberField value={seq.width} suffix={t("prepare.w")} onCommit={(width) => set({ width })} />
          <span className="faint">×</span>
          <NumberField value={seq.height} suffix={t("prepare.h")} onCommit={(height) => set({ height })} />
        </div>
        <div className="lbl">{t("newProject.fps")}</div>
        <div className="seg">
          {FPS.map((f) => (
            <button key={f} className={seq.fps === f ? "on" : ""} onClick={() => set({ fps: f })}>
              {f}
            </button>
          ))}
        </div>
        <div className="lbl">{t("prepare.background")}</div>
        <label className="field colorf">
          <input type="color" value={bg} onChange={(e) => setBg(e.target.value)} onBlur={() => bg !== seq.background && set({ background: bg.toUpperCase() })} />
          <span className="mono">{bg.toUpperCase()}</span>
        </label>
        <div className="hint">{t("prepare.undoHint")}</div>
      </section>
    </div>
  );
}
