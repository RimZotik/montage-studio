import { Captions, Scissors, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, ClipProps, Op, ProjectView } from "@/shared/ipc/bindings";
import { propsOf } from "@/render/TimelineComposition";
import { timecode } from "./time";

type Full = ReturnType<typeof propsOf>;

/** Ползунок, который применяет значение, когда его отпустили (один шаг отмены на движение). */
function Slider({ label, value, min, max, step, unit, scale = 1, onCommit }: { label: string; value: number; min: number; max: number; step: number; unit: string; scale?: number; onCommit: (v: number) => void }) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  return (
    <label className="sl">
      <span className="k">{label}</span>
      <input type="range" min={min} max={max} step={step} value={v} onChange={(e) => setV(Number(e.target.value))} onPointerUp={() => v !== value && onCommit(v)} onKeyUp={() => v !== value && onCommit(v)} />
      <span className="v mono">
        {Math.round(v * scale * 10) / 10}
        {unit}
      </span>
    </label>
  );
}

/** Свойства выбранного клипа: время, громкость, прозрачность, масштаб, сдвиг, плавность; текст
 *  субтитра. Пусто — подсказки и «Субтитр в точке». */
export function Inspector({ view, clip, frame, apply }: { view: ProjectView; clip: Clip | null; frame: number; apply: (ops: Op[], label: string) => Promise<boolean> }) {
  const { t } = useTranslation();
  const fps = view.project.sequence.fps;
  const [text, setText] = useState(clip?.text ?? "");
  useEffect(() => setText(clip?.text ?? ""), [clip?.id, clip?.text]);
  const captionTrack = view.tracks.find((tr) => tr.kind === "captions");

  const addCaption = () =>
    captionTrack && void apply([{ op: "addClip", track: captionTrack.id, kind: "caption", src: null, text: t("edit.captionText"), start: frame, duration: fps * 2, offset: null }], "caption");

  if (!clip) {
    return (
      <aside className="inspector">
        <div className="colhead">{t("edit.inspector")}</div>
        <p className="faint ihint">{t("edit.noSelection")}</p>
        {captionTrack && (
          <button className="btn" onClick={addCaption}>
            <Captions size={14} strokeWidth={1.75} />
            {t("edit.addCaption")}
          </button>
        )}
        <div className="keys">
          {(["space", "split", "delete", "ripple", "marker", "zoom"] as const).map((k) => (
            <div key={k}>
              <span className="mono">{t(`edit.keys.${k}.k`)}</span>
              {t(`edit.keys.${k}.v`)}
            </div>
          ))}
        </div>
      </aside>
    );
  }

  const p = propsOf(clip);
  const set = (patch: Partial<Full>) => void apply([{ op: "setProps", id: clip.id, props: { ...p, ...patch } as ClipProps }], "props");
  const visual = clip.kind !== "caption" && view.tracks.find((tr) => tr.id === clip.track)?.kind === "video";
  const audible = clip.kind === "media" && !/\.(png|jpe?g|webp|gif|svg|bmp)$/i.test(clip.src ?? "");

  return (
    <aside className="inspector">
      <div className="colhead">{t("edit.inspector")}</div>
      <div className="iname">{clip.kind === "caption" ? t("edit.caption") : (clip.src?.split("/").pop() ?? clip.text)}</div>
      <div className="itime mono">
        {timecode(clip.start, fps)} – {timecode(clip.start + clip.duration, fps)} · {(clip.duration / fps).toFixed(2)} {t("chat.sec")}
      </div>
      {clip.kind === "caption" && (
        <textarea className="field area" rows={3} value={text} onChange={(e) => setText(e.target.value)} onBlur={() => text !== clip.text && void apply([{ op: "setText", id: clip.id, text }], "text")} />
      )}
      {audible && <Slider label={t("edit.volume")} value={p.volume} min={0} max={2} step={0.01} unit="%" scale={100} onCommit={(volume) => set({ volume })} />}
      {visual && (
        <>
          <Slider label={t("edit.opacity")} value={p.opacity} min={0} max={1} step={0.01} unit="%" scale={100} onCommit={(opacity) => set({ opacity })} />
          <Slider label={t("edit.scale")} value={p.scale} min={0.1} max={4} step={0.01} unit="%" scale={100} onCommit={(scale) => set({ scale })} />
          <Slider label="X" value={p.x} min={-view.project.sequence.width} max={view.project.sequence.width} step={1} unit=" px" onCommit={(x) => set({ x })} />
          <Slider label="Y" value={p.y} min={-view.project.sequence.height} max={view.project.sequence.height} step={1} unit=" px" onCommit={(y) => set({ y })} />
        </>
      )}
      <Slider label={t("edit.fadeIn")} value={p.fadeIn} min={0} max={Math.min(fps * 3, clip.duration)} step={1} unit={` ${t("edit.fr")}`} onCommit={(fadeIn) => set({ fadeIn })} />
      <Slider label={t("edit.fadeOut")} value={p.fadeOut} min={0} max={Math.min(fps * 3, clip.duration)} step={1} unit={` ${t("edit.fr")}`} onCommit={(fadeOut) => set({ fadeOut })} />
      <div className="ibtns">
        <button className="btn" disabled={frame <= clip.start || frame >= clip.start + clip.duration} onClick={() => void apply([{ op: "splitClip", id: clip.id, at: frame }], "split")}>
          <Scissors size={14} strokeWidth={1.75} />
          {t("edit.split")}
        </button>
        <button className="btn" onClick={() => void apply([{ op: "deleteClips", ids: [clip.id], ripple: false }], "delete")}>
          <Trash2 size={14} strokeWidth={1.75} />
          {t("edit.delete")}
        </button>
      </div>
    </aside>
  );
}
