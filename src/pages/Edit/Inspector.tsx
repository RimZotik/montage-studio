import { BookmarkPlus, Captions, Eye, EyeOff, Scissors, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppliedEffect, type Clip, type ClipPropsPatch, type Op, type ProjectView, commands as ipc } from "@/shared/ipc/bindings";
import { propsOf } from "@/render/TimelineComposition";
import { ControlPanel, Slider, loc } from "@/shared/ui/ControlPanel";
import { timecode } from "./time";

type Full = ReturnType<typeof propsOf>;

/** Эффект на клипе: своя панель управления (копия из библиотеки, ее можно поменять только здесь),
 *  вкл/выкл, удалить, сохранить настройку в библиотеку. */
function EffectCard({ clip, fx, view, apply, lang }: { clip: Clip; fx: AppliedEffect; view: ProjectView; apply: (ops: Op[], label: string) => Promise<boolean>; lang: string }) {
  const { t } = useTranslation();
  const name = loc(fx.name, lang) || fx.effect;
  const save = () => {
    const controls = (Array.isArray(fx.controls) ? fx.controls : []).map((c: any) => (c.id && fx.params && c.id in fx.params ? { ...c, default: fx.params[c.id] } : c));
    const id = `${fx.effect.replace(/-my(-\d+)?$/, "")}-my`;
    void ipc.librarySave({ version: 1, id, kind: "effect", name: { ru: `${name} (моя настройка)`, en: `${name} (my preset)` }, description: "", targets: [], tags: [], controls, spec: fx.spec, engine: null, builtin: null, steps: null, each: null, author: "user", revision: 1 }, null);
  };
  return (
    <div className={`fxapplied ${fx.enabled === false ? "off" : ""}`}>
      <div className="fxappliedhead">
        <b>{name}</b>
        <button className="iconbtn" title={t("library.saveSetting")} onClick={save}>
          <BookmarkPlus size={13} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={t("library.toggle")} onClick={() => void apply([{ op: "setEffect", clip: clip.id, fx: fx.id ?? "", params: null, enabled: fx.enabled === false, controls: null, spec: null }], "effect")}>
          {fx.enabled === false ? <EyeOff size={13} strokeWidth={1.75} /> : <Eye size={13} strokeWidth={1.75} />}
        </button>
        <button className="iconbtn" title={t("edit.delete")} onClick={() => void apply([{ op: "removeEffect", clip: clip.id, fx: fx.id ?? "" }], "effect")}>
          <X size={13} strokeWidth={1.75} />
        </button>
      </div>
      <ControlPanel controls={fx.controls} values={fx.params ?? {}} tracks={view.tracks} onChange={(id, v) => void apply([{ op: "setEffect", clip: clip.id, fx: fx.id ?? "", params: { [id]: v }, enabled: null, controls: null, spec: null }], "effect")} />
    </div>
  );
}

/** Свойства выбранного клипа: время, громкость, прозрачность, масштаб, сдвиг, плавность; текст
 *  субтитра. Пусто — подсказки и «Субтитр в точке». */
export function Inspector({ view, clip, frame, apply }: { view: ProjectView; clip: Clip | null; frame: number; apply: (ops: Op[], label: string) => Promise<boolean> }) {
  const { t, i18n } = useTranslation();
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
  const set = (patch: Partial<Full>) => void apply([{ op: "setProps", id: clip.id, props: patch as ClipPropsPatch }], "props");
  const track = view.tracks.find((tr) => tr.id === clip.track);
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
      {track?.kind === "audio" && <Slider label={t("edit.trackVolume", { name: track.name })} value={track.volume ?? 1} min={0} max={2} step={0.01} unit="%" scale={100} onCommit={(volume) => void apply([{ op: "setTrack", id: track.id, muted: null, locked: null, volume }], "track")} />}
      {!!clip.effects?.length && <div className="ctlsec">{t("library.applied")}</div>}
      {clip.effects?.map((fx) => <EffectCard key={fx.id} clip={clip} fx={fx} view={view} apply={apply} lang={i18n.language} />)}
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
