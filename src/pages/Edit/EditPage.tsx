import { convertFileSrc } from "@tauri-apps/api/core";
import type { PlayerRef } from "@remotion/player";
import { Loader } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type MediaItem, type Op, type ProjectView, type SmartState, commands as ipc, events } from "@/shared/ipc/bindings";
import { typingTarget } from "@/app/commands";
import { Splitter, useWidths } from "@/shared/ui/Splitter";
import { MediaColumn } from "@/pages/Project/MediaColumn";
import { useDragMedia } from "./dragMedia";
import { EffectsColumn } from "./EffectsColumn";
import { Inspector } from "./Inspector";
import { Monitor } from "./Monitor";
import { useSetting } from "@/shared/settings/settings";
import { type EditTool, Timeline, type TimelineActions, type TimelineApi } from "./Timeline";

const WIDTHS = { media: 280, inspector: 300, timeline: 300 };
const FILL = { flex: 1, minHeight: 0 };
const noop = () => {};

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
  const [left, setLeft] = useSetting<"media" | "effects">("edit.leftTab", "media");
  const [tool, setTool] = useState<EditTool>("select");
  const [snapOn, setSnapOn] = useSetting("timeline.snap", true);
  const [pickedTrack, setPickedTrack] = useState<string | null>(null);
  const timelineApi = useRef<TimelineApi | null>(null);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    if (!note) return;
    const t = setTimeout(() => setNote(null), 3500);
    return () => clearTimeout(t);
  }, [note]);
  const drag = useDragMedia();

  const total = useMemo(() => Math.max(fps * 5, ...view.clips.map((c) => c.start + c.duration)), [view.clips, fps]);
  // Тяжелый файл играет в мониторе через облегченную копию; экспорт всегда берет исходник.
  const proxies = useMemo(() => new Map(media.filter((m) => m.proxy).map((m) => [m.path, m.proxy as string])), [media]);
  const srcOf = useCallback((p: string) => { const px = proxies.get(p); return convertFileSrc(px ?? `${view.path}\\${p.replaceAll("/", "\\")}`); }, [view.path, proxies]);

  useEffect(() => {
    void ipc.mediaList().then((r) => r.status === "ok" && setMedia(r.data));
    const off = events.mediaChanged.listen(() => void ipc.mediaList().then((r) => r.status === "ok" && setMedia(r.data)));
    return () => void off.then((f) => f());
  }, [mediaKey, view.path]);

  // Код проекта: при открытии, после правок и по сигналу Claude. Тот же текст — тот же объект, чтобы
  // монитор не пересобирал сцены без причины.
  const [smart, setSmart] = useState<SmartState | null>(null);
  const smartText = useRef("");
  const readSmart = useCallback(() => {
    void ipc.smartState().then((s) => {
      const text = JSON.stringify(s);
      if (text === smartText.current) return;
      smartText.current = text;
      setSmart(s);
    });
  }, []);
  useEffect(readSmart, [readSmart, view.clips, view.path]);
  useEffect(() => {
    const off = events.smartChanged.listen(readSmart);
    return () => void off.then((f) => f());
  }, [readSmart]);

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

  // Перемотка мышью дает десятки позиций в секунду, а видео успевает показать несколько: головка
  // движется сразу, плееру уходит только последняя позиция за кадр экрана.
  const seekWanted = useRef(0);
  const seekQueued = useRef(0);
  const seek = useCallback((f: number) => {
    seekWanted.current = f;
    setFrame(f);
    if (seekQueued.current) return;
    seekQueued.current = requestAnimationFrame(() => {
      seekQueued.current = 0;
      playerRef.current?.seekTo(seekWanted.current);
    });
  }, []);
  useEffect(() => () => cancelAnimationFrame(seekQueued.current), []);

  const selectClips = useCallback((ids: string[]) => {
    setSelected(ids);
    if (ids.length) setPickedTrack(null);
  }, []);
  const pickTrack = useCallback((id: string | null) => {
    setPickedTrack(id);
    if (id) setSelected([]);
  }, []);

  const apply = useCallback(
    async (ops: Op[], label: string) => {
      const r = await ipc.timelineApply(ops, label);
      if (r.status === "error") {
        onError(r.error);
        return false;
      }
      if ((ops[0]?.op === "addClip" || ops[0]?.op === "duplicateClips") && r.data[0]) setSelected(ops[0].op === "addClip" ? [r.data[0]] : r.data);
      return true;
    },
    [onError],
  );

  // Удаленные клипы не остаются выделенными.
  useEffect(() => setSelected((s) => s.filter((id) => view.clips.some((c) => c.id === id))), [view.clips]);

  const clipboard = useRef<string[]>([]);
  const unlocked = useCallback((id: string) => !view.tracks.find((t) => t.id === id)?.locked, [view.tracks]);
  const actions: TimelineActions = useMemo(
    () => ({
      split: () => {
        // Связанные режутся вместе: по одной операции на связку.
        const pool = view.clips.filter((c) => frame > c.start && frame < c.start + c.duration && unlocked(c.track) && (!selected.length || selected.includes(c.id)));
        const links = new Set<string>();
        const ops: Op[] = [];
        for (const c of pool) {
          if (c.link && links.has(c.link)) continue;
          if (c.link) links.add(c.link);
          ops.push({ op: "splitClip", id: c.id, at: frame });
        }
        if (ops.length) void apply(ops, "split");
      },
      remove: (ripple) => {
        if (selected.length) void apply([{ op: "deleteClips", ids: selected, ripple }], "delete");
      },
      marker: (at) => void apply([{ op: "addMarker", frame: at ?? frame, label: "" }], "marker"),
      text: (at) => {
        const track = view.tracks.find((t) => t.kind === "captions");
        if (track) void apply([{ op: "addClip", track: track.id, kind: "caption", src: null, text: t("edit.caption"), start: at ?? frame, duration: fps * 2, offset: null }], "text");
      },
      duplicate: () => {
        if (selected.length) void apply([{ op: "duplicateClips", ids: selected, at: null }], "duplicate");
      },
      selectAll: () => setSelected(view.clips.map((c) => c.id)),
      copy: () => {
        clipboard.current = selected;
      },
      paste: () => {
        const ids = clipboard.current.filter((id) => view.clips.some((c) => c.id === id));
        if (ids.length) void apply([{ op: "duplicateClips", ids, at: frame }], "paste");
      },
    }),
    [apply, fps, frame, selected, t, unlocked, view.clips, view.tracks],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Открытое окно поверх страницы забирает клавиши себе.
      if (document.querySelector(".overlay")) return;
      if (typingTarget(e.target)) return;
      if (e.altKey) return;
      if (e.ctrlKey) {
        if (e.code === "KeyA") {
          e.preventDefault();
          actions.selectAll();
        } else if (e.code === "KeyD") {
          e.preventDefault();
          actions.duplicate();
        } else if (e.code === "KeyC") {
          actions.copy();
        } else if (e.code === "KeyV") {
          e.preventDefault();
          actions.paste();
        } else if (e.code === "KeyK") {
          e.preventDefault();
          actions.split();
        }
        return;
      }
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
          actions.split();
          break;
        case "Delete":
        case "Backspace":
          actions.remove(e.shiftKey);
          break;
        case "KeyM":
          actions.marker();
          break;
        case "KeyV":
          setTool("select");
          break;
        case "KeyC":
          setTool("razor");
          break;
        case "KeyN":
          setSnapOn((s) => !s);
          break;
        case "KeyT":
          actions.text();
          break;
        case "Escape":
          setTool("select");
          setSelected([]);
          setPickedTrack(null);
          break;
        case "Equal":
        case "NumpadAdd":
          timelineApi.current?.zoomBy(1.25);
          break;
        case "Minus":
        case "NumpadSubtract":
          timelineApi.current?.zoomBy(1 / 1.25);
          break;
        default:
          return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [actions, frame, fps, seek, total]);

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

  const openEffects = useCallback(() => setLeft("effects"), []);

  // Свойства — у клипа, по которому щелкнули (у связки выделены оба, первым — он).
  const first = view.clips.find((c) => c.id === selected[0]) ?? null;
  const clip = first && selected.every((id) => id === first.id || (!!first.link && view.clips.find((c) => c.id === id)?.link === first.link)) ? first : null;

  return (
    <div className="editpage">
      <div className="edtop">
        <div className="leftcol" style={{ width: w.media }}>
          <div className="lefttabs">
            {(["media", "effects"] as const).map((k) => (
              <button key={k} className={left === k ? "on" : ""} onClick={() => setLeft(k)}>
                {t(`edit.left.${k}`)}
              </button>
            ))}
          </div>
          {left === "media" ? <MediaColumn projectPath={view.path} reloadKey={mediaKey} selected={null} onSelect={noop} onError={onError} onImport={onImport} style={FILL} compact /> : <EffectsColumn view={view} selected={selected} pickedTrack={pickedTrack} frame={frame} onError={onError} style={FILL} />}
        </div>
        <Splitter width={w.media} min={200} max={480} side="left" onChange={(v) => setW("media", v)} onReset={() => setW("media", WIDTHS.media)} />
        <Monitor view={view} total={total} playerRef={playerRef} srcOf={srcOf} frame={frame} playing={playing} smart={smart} />
        <Splitter width={w.inspector} min={240} max={480} side="right" onChange={(v) => setW("inspector", v)} onReset={() => setW("inspector", WIDTHS.inspector)} />
        <div style={{ width: w.inspector, display: "flex", minHeight: 0 }}>
          <Inspector view={view} clip={clip} frame={frame} apply={apply} onError={onError} selectedCount={selected.length} pickedTrack={pickedTrack} />
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
        <Timeline view={view} frame={frame} total={total} zoom={zoom} onZoom={setZoom} onSeek={seek} selected={selected} onSelect={selectClips} apply={apply} media={media} tool={tool} onTool={setTool} snapOn={snapOn} onSnap={setSnapOn} actions={actions} onOpenEffects={openEffects} onNote={setNote} onError={onError} pickedTrack={pickedTrack} onPickTrack={pickTrack} apiRef={timelineApi} />
        {note && <div className="tlnote">{note}</div>}
      </div>
      {drag && (
        <div className="dragghost" style={{ left: drag.x + 12, top: drag.y + 12 }}>
          {drag.payload.name}
        </div>
      )}
    </div>
  );
}
