import { Captions, Layers, Plus, Type, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type AppliedEffect, type LibraryRow, type Op, type ProjectView, type Track, commands as ipc, events } from "@/shared/ipc/bindings";
import { ControlPanel, Slider, loc } from "@/shared/ui/ControlPanel";

const TARGET: Record<string, string> = { captions: "text", video: "video", audio: "audio" };

/** Свойства дорожки: что действует на все ее клипы сразу. Эффекты дорожки получают и будущие клипы;
 *  эффекты, уже лежащие на клипах, правятся здесь одним действием на все. */
export function TrackPanel({ view, track, apply, onError }: { view: ProjectView; track: Track; apply: (ops: Op[], label: string) => Promise<boolean>; onError: (e: AppError) => void }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [lib, setLib] = useState<LibraryRow[]>([]);
  const load = useCallback(() => void ipc.libraryView().then((r) => (r.status === "ok" ? setLib(r.data.items) : onError(r.error))), [onError]);
  useEffect(() => {
    load();
    const off = events.libraryChanged.listen(load);
    return () => void off.then((f) => f());
  }, [load]);

  const clips = useMemo(() => view.clips.filter((c) => c.track === track.id), [view.clips, track.id]);
  // Эффекты клипов по элементу библиотеки: один и тот же у многих клипов — одна строка.
  const shared = useMemo(() => {
    const seen = new Map<string, { fx: AppliedEffect; count: number }>();
    for (const c of clips) for (const fx of c.effects ?? []) seen.set(fx.effect, { fx: seen.get(fx.effect)?.fx ?? fx, count: (seen.get(fx.effect)?.count ?? 0) + 1 });
    return [...seen.values()];
  }, [clips]);
  const choices = lib.filter((r) => r.item.kind === "effect" && (r.item.targets ?? []).includes(TARGET[track.kind] ?? "video") && !(track.effects ?? []).some((f) => f.effect === r.item.id));
  const first = clips[0];
  const select = { track: track.id };

  return (
    <aside className="col inspector trackpanel">
      <div className="colhead">
        {t("edit.trackPanel.title")} {track.name}
      </div>
      <p className="faint fxdesc">{t(`edit.trackPanel.hint.${track.kind}`)}</p>

      {track.kind === "captions" && (
        <div className="segrow">
          {(["burned", "file"] as const).map((m) => (
            <button key={m} className={(track.captionMode ?? "burned") === m ? "on" : ""} onClick={() => void apply([{ op: "setTrack", id: track.id, muted: null, locked: null, volume: null, captionMode: m }], "track")}>
              {m === "burned" ? <Type size={13} strokeWidth={1.75} /> : <Captions size={13} strokeWidth={1.75} />}
              {t(`edit.trackPanel.mode.${m}`)}
            </button>
          ))}
        </div>
      )}

      <div className="ctlsec">{t("edit.trackPanel.all", { count: clips.length })}</div>
      {!!first && track.kind !== "audio" && (
        <>
          <Slider label={t("edit.scale")} value={first.props?.scale ?? 1} min={0.1} max={4} step={0.01} unit="%" scale={100} onCommit={(scale) => void apply([{ op: "editWhere", select, props: { scale } }], "track")} />
          <Slider label="Y" value={first.props?.y ?? 0} min={-view.project.sequence.height} max={view.project.sequence.height} step={1} unit=" px" onCommit={(y) => void apply([{ op: "editWhere", select, props: { y } }], "track")} />
        </>
      )}
      {track.kind === "audio" && <Slider label={t("edit.volume")} value={track.volume ?? 1} min={0} max={2} step={0.01} unit="%" scale={100} onCommit={(volume) => void apply([{ op: "setTrack", id: track.id, muted: null, locked: null, volume, captionMode: null }], "track")} />}

      <div className="ctlsec">{t("edit.trackPanel.effects")}</div>
      {(track.effects ?? []).map((fx) => (
        <div key={fx.id} className={`fxapplied ${fx.enabled === false ? "off" : ""}`}>
          <div className="fxappliedhead">
            <b>{loc(fx.name, lang) || fx.effect}</b>
            <button className="iconbtn" title={t("edit.delete")} onClick={() => void apply([{ op: "removeTrackEffect", track: track.id, fx: fx.id ?? "" }], "effect")}>
              <X size={13} strokeWidth={1.75} />
            </button>
          </div>
          <ControlPanel controls={fx.controls} values={fx.params ?? {}} tracks={view.tracks} onChange={(id, v) => void apply([{ op: "setTrackEffect", track: track.id, fx: fx.id ?? "", params: { [id]: v } }], "effect")} />
        </div>
      ))}
      <label className="addfx">
        <Plus size={14} strokeWidth={1.75} />
        <select
          className="field"
          value=""
          onChange={(e) => {
            if (e.target.value) void ipc.trackEffectAdd(e.target.value, {}, track.id).then((r) => r.status === "error" && onError(r.error));
          }}
        >
          <option value="">{t("edit.trackPanel.add")}</option>
          {choices.map((r) => (
            <option key={r.item.id} value={r.item.id}>
              {loc(r.item.name, lang)}
            </option>
          ))}
        </select>
      </label>

      {!!shared.length && <div className="ctlsec">{t("edit.trackPanel.onClips")}</div>}
      {shared.map(({ fx, count }) => (
        <div key={fx.effect} className="fxapplied">
          <div className="fxappliedhead">
            <b>
              {loc(fx.name, lang) || fx.effect} · {count}
            </b>
            <button className="iconbtn" title={t("edit.trackPanel.toTrack")} onClick={() => void apply([{ op: "addTrackEffect", track: track.id, effect: fx }, { op: "editWhere", select, removeEffect: fx.effect }], "effect")}>
              <Layers size={13} strokeWidth={1.75} />
            </button>
            <button className="iconbtn" title={t("edit.trackPanel.removeAll")} onClick={() => void apply([{ op: "editWhere", select, removeEffect: fx.effect }], "effect")}>
              <X size={13} strokeWidth={1.75} />
            </button>
          </div>
          <ControlPanel controls={fx.controls} values={fx.params ?? {}} tracks={view.tracks} onChange={(id, v) => void apply([{ op: "editWhere", select, effect: fx.effect, params: { [id]: v } }], "effect")} />
        </div>
      ))}
    </aside>
  );
}
