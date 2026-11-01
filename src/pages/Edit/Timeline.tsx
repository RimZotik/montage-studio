import { convertFileSrc } from "@tauri-apps/api/core";
import { Eye, EyeOff, Lock, LockOpen, Volume2, VolumeX } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, MediaItem, Op, ProjectView, Track } from "@/shared/ipc/bindings";
import { setDropTarget } from "./dragMedia";
import { snap, timecode } from "./time";

type Props = {
  view: ProjectView;
  frame: number;
  total: number;
  zoom: number;
  onZoom: (z: number) => void;
  onSeek: (f: number) => void;
  selected: string[];
  onSelect: (ids: string[]) => void;
  apply: (ops: Op[], label: string) => Promise<boolean>;
  media: MediaItem[];
};

type Drag = { mode: "move" | "trimL" | "trimR"; clip: Clip; x0: number; track: string; start: number; duration: number; offset: number; moved: boolean };

const HEAD = 132;
const SNAP_PX = 8;

/** Дорожка подходит для файла медиа: картинка и видео — на видео, звук — на звук (видео можно и
 *  на звуковую дорожку — возьмется звук). */
function fits(track: Track, item: MediaItem): boolean {
  if (track.kind === "video") return item.category === "footage" || item.category === "images";
  if (track.kind === "audio") return item.category !== "images";
  return false;
}

/** Таймлайн: дорожки, клипы, линейка, головка; перенос, обрезка краев, привязка, перенос файлов
 *  из медиа. Правки уходят пакетами операций в ядро (одна отмена на действие). */
export function Timeline({ view, frame, total, zoom, onZoom, onSeek, selected, onSelect, apply, media }: Props) {
  const { t } = useTranslation();
  const fps = view.project.sequence.fps;
  const scroller = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const byPath = useMemo(() => new Map(media.map((m) => [m.path, m])), [media]);
  const width = Math.max(total + fps * 10, fps * 40) * zoom;

  const frameAt = useCallback((clientX: number) => {
    const el = scroller.current;
    if (!el) return 0;
    const r = el.getBoundingClientRect();
    return Math.max(0, Math.round((clientX - r.left - HEAD + el.scrollLeft) / zoom));
  }, [zoom]);

  const snapPoints = useCallback(
    (except: string) => [0, frame, ...view.markers.map((m) => m.frame), ...view.clips.filter((c) => c.id !== except).flatMap((c) => [c.start, c.start + c.duration])],
    [frame, view.clips, view.markers],
  );

  // Файл из медиа, отпущенный над дорожкой, — новый клип с начала под курсором.
  useEffect(() => {
    setDropTarget((item, x, y) => {
      const lane = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-track]");
      const track = view.tracks.find((tr) => tr.id === lane?.dataset.track) ?? view.tracks.find((tr) => fits(tr, item));
      if (!track || !fits(track, item)) return;
      const duration = item.category === "images" ? fps * 3 : Math.max(1, Math.round((item.probe?.durationS ?? 5) * fps));
      const start = snap(frameAt(x), snapPoints(""), SNAP_PX / zoom);
      void apply([{ op: "addClip", track: track.id, kind: "media", src: item.path, text: null, start, duration, offset: null }], "add");
    });
    return () => setDropTarget(null);
  }, [view.tracks, fps, frameAt, snapPoints, zoom, apply]);

  const onClipDown = (e: React.PointerEvent, clip: Clip, mode: Drag["mode"]) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    if (!selected.includes(clip.id)) onSelect(e.shiftKey ? [...selected, clip.id] : [clip.id]);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    setDrag({ mode, clip, x0: e.clientX, track: clip.track, start: clip.start, duration: clip.duration, offset: clip.offset ?? 0, moved: false });
  };

  const onMove = (e: React.PointerEvent) => {
    if (!drag) return;
    const df = Math.round((e.clientX - drag.x0) / zoom);
    const c = drag.clip;
    const pts = snapPoints(c.id);
    const r = SNAP_PX / zoom;
    const source = c.src ? byPath.get(c.src)?.probe?.durationS : null;
    // Длина файла ограничивает растяжение клипа вправо (у текста и заливки — без ограничения).
    const maxDur = c.kind === "media" && source ? Math.round(source * fps) - (c.offset ?? 0) : Infinity;
    let next = { ...drag, moved: drag.moved || Math.abs(e.clientX - drag.x0) > 2 };
    if (drag.mode === "move") {
      let start = Math.max(0, c.start + df);
      const end = snap(start + c.duration, pts, r);
      start = end !== start + c.duration ? end - c.duration : snap(start, pts, r);
      const lane = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-track]");
      const over = view.tracks.find((tr) => tr.id === lane?.dataset.track);
      const fromKind = view.tracks.find((tr) => tr.id === c.track)?.kind;
      next = { ...next, start: Math.max(0, start), track: over && over.kind === fromKind ? over.id : drag.track };
    } else if (drag.mode === "trimL") {
      const minStart = Math.max(0, c.start - (c.offset ?? 0));
      const start = Math.min(Math.max(snap(c.start + df, pts, r), c.kind === "media" ? minStart : 0), c.start + c.duration - 1);
      next = { ...next, start, duration: c.duration + (c.start - start), offset: Math.max(0, (c.offset ?? 0) + (start - c.start)) };
    } else {
      const end = snap(c.start + c.duration + df, pts, r);
      next = { ...next, duration: Math.min(Math.max(1, end - c.start), maxDur) };
    }
    setDrag(next);
  };

  const onUp = () => {
    if (!drag) return;
    const d = drag;
    setDrag(null);
    if (!d.moved) return;
    const c = d.clip;
    if (d.mode === "move") {
      if (d.start !== c.start || d.track !== c.track) void apply([{ op: "moveClip", id: c.id, track: d.track !== c.track ? d.track : null, start: d.start }], "move");
    } else {
      void apply([{ op: "trimClip", id: c.id, start: d.start, duration: d.duration, offset: c.kind === "media" ? d.offset : null }], "trim");
    }
  };

  // Колесо с Ctrl — масштаб к курсору; без — прокрутка.
  const onWheel = (e: React.WheelEvent) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    const el = scroller.current;
    if (!el) return;
    const at = frameAt(e.clientX);
    const z = Math.min(40, Math.max(0.05, zoom * (e.deltaY < 0 ? 1.2 : 1 / 1.2)));
    onZoom(z);
    requestAnimationFrame(() => {
      el.scrollLeft = at * z - (e.clientX - el.getBoundingClientRect().left - HEAD);
    });
  };

  // Головка в видимой области при воспроизведении.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const x = frame * zoom;
    if (x < el.scrollLeft || x > el.scrollLeft + el.clientWidth - HEAD - 40) el.scrollLeft = Math.max(0, x - 80);
  }, [frame, zoom]);

  const step = fps * (zoom * fps > 160 ? 1 : zoom * fps > 40 ? 5 : zoom * fps > 12 ? 10 : 30);
  const ticks = [];
  for (let f = 0; f * zoom < width; f += step) ticks.push(f);

  const scrub = (e: React.PointerEvent) => {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    onSeek(frameAt(e.clientX));
  };

  return (
    <div className="timeline" ref={scroller} onWheel={onWheel} onPointerMove={onMove} onPointerUp={onUp}>
      <div className="tl-inner" style={{ width: width + HEAD }}>
        <div className="tl-ruler" onPointerDown={scrub} onPointerMove={(e) => e.buttons === 1 && !drag && onSeek(frameAt(e.clientX))}>
          <div className="tl-corner mono">{timecode(frame, fps)}</div>
          {ticks.map((f) => (
            <span key={f} className="tick" style={{ left: HEAD + f * zoom }}>
              {timecode(f, fps).slice(0, 5)}
            </span>
          ))}
          {view.markers.map((m) => (
            <span key={m.frame} className="marker" style={{ left: HEAD + m.frame * zoom }} title={m.label} />
          ))}
        </div>
        {view.tracks.map((tr) => (
          <div key={tr.id} className={`lane ${tr.kind} ${tr.muted ? "muted" : ""}`} data-track={tr.id} onPointerDown={(e) => e.target === e.currentTarget && onSelect([])}>
            <div className="lhead">
              <b className={`swatch ${tr.kind}`} />
              <span className="lname">{tr.name}</span>
              <span className="lkind">{t(`edit.track.${tr.kind}`)}</span>
              <button className="iconbtn" title={t("edit.mute")} onClick={() => void apply([{ op: "setTrack", id: tr.id, muted: !tr.muted, locked: null }], "track")}>
                {tr.kind === "audio" ? tr.muted ? <VolumeX size={12} strokeWidth={1.75} /> : <Volume2 size={12} strokeWidth={1.75} /> : tr.muted ? <EyeOff size={12} strokeWidth={1.75} /> : <Eye size={12} strokeWidth={1.75} />}
              </button>
              <button className="iconbtn" title={t("edit.lock")} onClick={() => void apply([{ op: "setTrack", id: tr.id, muted: null, locked: !tr.locked }], "track")}>
                {tr.locked ? <Lock size={12} strokeWidth={1.75} /> : <LockOpen size={12} strokeWidth={1.75} />}
              </button>
            </div>
            {view.clips
              .filter((c) => (drag?.clip.id === c.id ? drag.track : c.track) === tr.id)
              .map((c) => {
                const live = drag?.clip.id === c.id ? drag : null;
                const start = live ? live.start : c.start;
                const duration = live ? live.duration : c.duration;
                const m = c.src ? byPath.get(c.src) : undefined;
                const label = c.kind === "caption" ? c.text : c.kind === "solid" ? c.text : (m?.name ?? c.src?.split("/").pop());
                const bg = m?.thumb ? `url("${convertFileSrc(m.thumb)}")` : undefined;
                return (
                  <div
                    key={c.id}
                    className={`clip ${tr.kind} ${selected.includes(c.id) ? "sel" : ""}`}
                    style={{ left: HEAD + start * zoom, width: Math.max(4, duration * zoom), backgroundImage: bg }}
                    onPointerDown={(e) => onClipDown(e, c, "move")}
                    title={`${label ?? ""} · ${timecode(start, fps)} – ${timecode(start + duration, fps)}`}
                  >
                    <span className="h l" onPointerDown={(e) => onClipDown(e, c, "trimL")} />
                    <span className="cl">{label}</span>
                    <span className="h r" onPointerDown={(e) => onClipDown(e, c, "trimR")} />
                  </div>
                );
              })}
          </div>
        ))}
        <div className="playhead" style={{ left: HEAD + frame * zoom }} />
      </div>
      {view.clips.length === 0 && <div className="tl-empty">{t("edit.empty")}</div>}
    </div>
  );
}
