import { convertFileSrc } from "@tauri-apps/api/core";
import type { PlayerRef } from "@remotion/player";
import { Loader } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type MediaItem, type Op, type ProjectView, commands as ipc, events } from "@/shared/ipc/bindings";
import { typingTarget } from "@/app/commands";
import { Splitter, useWidths } from "@/shared/ui/Splitter";
import { MediaColumn } from "@/pages/Project/MediaColumn";
import { useDragMedia } from "./dragMedia";
import { Inspector } from "./Inspector";
import { Monitor } from "./Monitor";
import { Timeline } from "./Timeline";

const WIDTHS = { media: 280, inspector: 300, timeline: 300 };

/** «Монтаж»: медиа | монитор | свойства, снизу таймлайн. Клавиши: пробел, стрелки, S, Delete,
 *  Shift+Delete, M, +/−. */
export function EditPage({ view, onError, onImport, mediaKey, claudeBusy }: { view: ProjectView; onError: (e: AppError) => void; onImport: (files: string[]) => void; mediaKey: number; claudeBusy: boolean }) {
  const { t } = useTranslation();
  const fps = view.project.sequence.fps;
  const playerRef = useRef<PlayerRef | null>(null);
  const [frame, setFrame] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [zoom, setZoom] = useState(2);
  const [media, setMedia] = useState<MediaItem[]>([]);
  const [w, setW] = useWidths("edit", WIDTHS);
  const drag = useDragMedia();

  const total = useMemo(() => Math.max(fps * 5, ...view.clips.map((c) => c.start + c.duration)), [view.clips, fps]);
  const srcOf = useCallback((p: string) => convertFileSrc(`${view.path}\\${p.replaceAll("/", "\\")}`), [view.path]);

  useEffect(() => {
    void ipc.mediaList().then((r) => r.status === "ok" && setMedia(r.data));
    const off = events.mediaChanged.listen(() => void ipc.mediaList().then((r) => r.status === "ok" && setMedia(r.data)));
    return () => void off.then((f) => f());
  }, [mediaKey, view.path]);

  // Головка и состояние воспроизведения — из плеера.
  useEffect(() => {
    const p = playerRef.current;
    if (!p) return;
    const onFrame = (e: { detail: { frame: number } }) => setFrame(e.detail.frame);
    const onPlay = () => setPlaying(true);
    const onPause = () => setPlaying(false);
    p.addEventListener("frameupdate", onFrame);
    p.addEventListener("play", onPlay);
    p.addEventListener("pause", onPause);
    return () => {
      p.removeEventListener("frameupdate", onFrame);
      p.removeEventListener("play", onPlay);
      p.removeEventListener("pause", onPause);
    };
  }, []);

  const seek = useCallback((f: number) => {
    playerRef.current?.seekTo(f);
    setFrame(f);
  }, []);

  const apply = useCallback(
    async (ops: Op[], label: string) => {
      const r = await ipc.timelineApply(ops, label);
      if (r.status === "error") {
        onError(r.error);
        return false;
      }
      if (ops[0]?.op === "addClip" && r.data[0]) setSelected([r.data[0]]);
      return true;
    },
    [onError],
  );

  // Удаленные клипы не остаются выделенными.
  useEffect(() => setSelected((s) => s.filter((id) => view.clips.some((c) => c.id === id))), [view.clips]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typingTarget(e.target) || e.ctrlKey || e.altKey) {
        if (e.ctrlKey && e.code === "KeyK") e.preventDefault();
        else return;
      }
      const under = view.clips.filter((c) => frame > c.start && frame < c.start + c.duration);
      const targets = selected.length ? view.clips.filter((c) => selected.includes(c.id) && frame > c.start && frame < c.start + c.duration) : under;
      switch (e.code) {
        case "Space":
          e.preventDefault();
          playerRef.current?.toggle();
          break;
        case "ArrowLeft":
          seek(Math.max(0, frame - (e.shiftKey ? fps : 1)));
          break;
        case "ArrowRight":
          seek(frame + (e.shiftKey ? fps : 1));
          break;
        case "Home":
          seek(0);
          break;
        case "End":
          seek(total);
          break;
        case "KeyS":
        case "KeyK":
          if (targets.length) void apply(targets.map((c) => ({ op: "splitClip", id: c.id, at: frame })), "split");
          break;
        case "Delete":
        case "Backspace":
          if (selected.length) void apply([{ op: "deleteClips", ids: selected, ripple: e.shiftKey }], "delete");
          break;
        case "KeyM":
          void apply([{ op: "addMarker", frame, label: "" }], "marker");
          break;
        case "Equal":
        case "NumpadAdd":
          setZoom((z) => Math.min(40, z * 1.25));
          break;
        case "Minus":
        case "NumpadSubtract":
          setZoom((z) => Math.max(0.05, z / 1.25));
          break;
        default:
          return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [apply, frame, fps, seek, selected, total, view.clips]);

  // Команды Claude интерфейсу: головка и выделение.
  useEffect(() => {
    const off = events.uiCommand.listen(({ payload: c }) => {
      if (c.kind === "seek") seek(c.frame);
      if (c.kind === "select") setSelected(c.ids);
    });
    return () => void off.then((f) => f());
  }, [seek]);

  // Что видит Claude в сводке проекта: головка, выделение, вкладка.
  useEffect(() => {
    const t = setTimeout(() => void ipc.uiStateReport({ frame, selected, tab: "edit" }), 250);
    return () => clearTimeout(t);
  }, [frame, selected]);

  // Свойства — у клипа, по которому щелкнули (у связки выделены оба, первым — он).
  const first = view.clips.find((c) => c.id === selected[0]) ?? null;
  const clip = first && selected.every((id) => id === first.id || (!!first.link && view.clips.find((c) => c.id === id)?.link === first.link)) ? first : null;

  return (
    <div className="editpage">
      <div className="edtop">
        <MediaColumn projectPath={view.path} reloadKey={mediaKey} selected={null} onSelect={() => {}} onError={onError} onImport={onImport} style={{ width: w.media }} compact />
        <Splitter width={w.media} min={200} max={480} side="left" onChange={(v) => setW("media", v)} onReset={() => setW("media", WIDTHS.media)} />
        <Monitor view={view} total={total} playerRef={playerRef} srcOf={srcOf} frame={frame} playing={playing} />
        <Splitter width={w.inspector} min={240} max={480} side="right" onChange={(v) => setW("inspector", v)} onReset={() => setW("inspector", WIDTHS.inspector)} />
        <div style={{ width: w.inspector, display: "flex", minHeight: 0 }}>
          <Inspector view={view} clip={clip} frame={frame} apply={apply} />
        </div>
      </div>
      <Splitter width={w.timeline} min={160} max={640} side="right" vertical onChange={(v) => setW("timeline", v)} onReset={() => setW("timeline", WIDTHS.timeline)} />
      <div style={{ height: w.timeline, display: "flex", minHeight: 0, position: "relative" }}>
        {claudeBusy && (
          <div className="claudelock">
            <span className="ic">
              <Loader size={14} strokeWidth={1.75} className="spin" />
              {t("edit.claudeBusy")}
            </span>
            <button className="btn" onClick={() => void ipc.chatStop()}>
              {t("chat.stop")}
            </button>
          </div>
        )}
        <Timeline view={view} frame={frame} total={total} zoom={zoom} onZoom={setZoom} onSeek={seek} selected={selected} onSelect={setSelected} apply={apply} media={media} />
      </div>
      {drag && (
        <div className="dragghost" style={{ left: drag.x + 12, top: drag.y + 12 }}>
          {drag.item.name}
        </div>
      )}
    </div>
  );
}
