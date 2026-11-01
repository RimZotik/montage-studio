import { convertFileSrc } from "@tauri-apps/api/core";
import { Eye, EyeOff, Link2, Lock, LockOpen, Sparkles, Volume2, VolumeX } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Clip, MediaItem, Op, ProjectView, Track } from "@/shared/ipc/bindings";
import { setDropTarget, useDragMedia } from "./dragMedia";
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
  // Отпущенная правка до ответа ядра: клип остается там, где его отпустили (без рывка назад).
  const [pending, setPending] = useState<{ id: string; track: string; start: number; duration: number } | null>(null);
  useEffect(() => setPending(null), [view.clips]);
  const mediaDrag = useDragMedia();
  const byPath = useMemo(() => new Map(media.map((m) => [m.path, m])), [media]);
  const order = useMemo(() => new Map(view.clips.map((c, i) => [c.id, i])), [view.clips]);
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

  /** Куда ляжет файл из медиа под курсором: только на подходящую дорожку под курсором. */
  const dropAt = useCallback(
    (item: MediaItem, x: number, y: number) => {
      const lane = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-track]");
      const track = view.tracks.find((tr) => tr.id === lane?.dataset.track);
      if (!track || !fits(track, item) || track.locked) return null;
      const duration = item.category === "images" ? fps * 3 : Math.max(1, Math.round((item.probe?.durationS ?? 5) * fps));
      const start = snap(frameAt(x), snapPoints(""), SNAP_PX / zoom);
      return { track: track.id, start, duration };
    },
    [view.tracks, fps, frameAt, snapPoints, zoom],
  );

  // Файл из медиа, отпущенный над дорожкой, — новый клип; мимо дорожки — ничего.
  useEffect(() => {
    setDropTarget((item, x, y) => {
      const at = dropAt(item, x, y);
      if (!at) return;
      const onVideo = view.tracks.find((tr) => tr.id === at.track)?.kind === "video";
      const audio = view.tracks.find((tr) => tr.kind === "audio" && !tr.locked);
      if (onVideo && item.category === "footage" && item.probe?.audio !== false && audio)
        void apply([{ op: "addLinked", videoTrack: at.track, audioTrack: audio.id, src: item.path, start: at.start, duration: at.duration, offset: null }], "add");
      else void apply([{ op: "addClip", track: at.track, kind: "media", src: item.path, text: null, start: at.start, duration: at.duration, offset: null }], "add");
    });
    return () => setDropTarget(null);
  }, [dropAt, apply, view.tracks]);
  const ghost = mediaDrag ? dropAt(mediaDrag.item, mediaDrag.x, mediaDrag.y) : null;

  const onClipDown = (e: React.PointerEvent, clip: Clip, mode: Drag["mode"]) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    const group = clip.link ? [clip.id, ...view.clips.filter((c) => c.link === clip.link && c.id !== clip.id).map((c) => c.id)] : [clip.id];
    if (!selected.includes(clip.id)) onSelect(e.shiftKey ? [...selected, ...group] : group);
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
    // Пока мышь почти не сдвинулась — клип на месте (без рывка к ближней точке привязки).
    if (!drag.moved && Math.abs(e.clientX - drag.x0) <= 3 && drag.mode === "move") return;
    let next = { ...drag, moved: true };
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
    setPending({ id: c.id, track: d.track, start: d.start, duration: d.duration });
    const done = (ok: boolean) => !ok && setPending(null);
    if (d.mode === "move") {
      if (d.start !== c.start || d.track !== c.track) void apply([{ op: "moveClip", id: c.id, track: d.track !== c.track ? d.track : null, start: d.start }], "move").then(done);
      else setPending(null);
    } else {
      void apply([{ op: "trimClip", id: c.id, start: d.start, duration: d.duration, offset: c.kind === "media" ? d.offset : null }], "trim").then(done);
    }
  };

  // Колесо — по времени; Shift+колесо — по дорожкам; Ctrl или Alt+колесо — масштаб к курсору.
  const onWheel = (e: React.WheelEvent) => {
    const el = scroller.current;
    if (!el) return;
    if (!e.ctrlKey && !e.altKey) {
      if (e.shiftKey) return;
      el.scrollLeft += e.deltaY + e.deltaX;
      return;
    }
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

  // Видимый участок: на большом таймлайне рисуются только клипы рядом с ним.
  const [view0, setView0] = useState({ left: 0, width: 4000 });
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const update = () => setView0({ left: el.scrollLeft, width: el.clientWidth });
    update();
    let raf = 0;
    const onScroll = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(update);
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", onScroll);
      ro.disconnect();
      cancelAnimationFrame(raf);
    };
  }, []);
  const margin = view0.width;
  const fromF = Math.floor((view0.left - margin) / zoom);
  const toF = Math.ceil((view0.left + view0.width + margin) / zoom);
  const visibleKey = `${Math.floor(fromF / (fps * 5))}:${Math.floor(toF / (fps * 5))}`;

  const step = fps * (zoom * fps > 160 ? 1 : zoom * fps > 40 ? 5 : zoom * fps > 12 ? 10 : 30);
  const ticks = [];
  for (let f = 0; f * zoom < width; f += step) ticks.push(f);

  // Клипы не перерисовываются от движения головки: при воспроизведении меняется только она.
  const [cullFrom, cullTo] = [Math.floor(fromF / (fps * 5)) * fps * 5, (Math.floor(toF / (fps * 5)) + 1) * fps * 5];
  const lanes = useMemo(
    () => (
      <>
        {view.tracks.map((tr) => (
          <div key={tr.id} className={`lane ${tr.kind} ${tr.muted ? "muted" : ""}`} data-track={tr.id} onPointerDown={(e) => e.target === e.currentTarget && onSelect([])}>
            <div className="lhead">
              <b className={`swatch ${tr.kind}`} />
              <span className="lname">{tr.name}</span>
              <span className="lkind">{t(`edit.track.${tr.kind}`)}</span>
              <button className="iconbtn" title={t("edit.mute")} onClick={() => void apply([{ op: "setTrack", id: tr.id, muted: !tr.muted, locked: null, volume: null }], "track")}>
                {tr.kind === "audio" ? tr.muted ? <VolumeX size={12} strokeWidth={1.75} /> : <Volume2 size={12} strokeWidth={1.75} /> : tr.muted ? <EyeOff size={12} strokeWidth={1.75} /> : <Eye size={12} strokeWidth={1.75} />}
              </button>
              <button className="iconbtn" title={t("edit.lock")} onClick={() => void apply([{ op: "setTrack", id: tr.id, muted: null, locked: !tr.locked, volume: null }], "track")}>
                {tr.locked ? <Lock size={12} strokeWidth={1.75} /> : <LockOpen size={12} strokeWidth={1.75} />}
              </button>
            </div>
            {view.clips
              .filter((c) => (drag?.clip.id === c.id ? drag.track : pending?.id === c.id ? pending.track : c.track) === tr.id)
              // Вне видимого участка клип не рисуется (кроме того, что сейчас тянут).
              .filter((c) => c.start + c.duration >= cullFrom && c.start <= cullTo || drag?.clip.id === c.id || selected.includes(c.id))
              .map((c) => {
                const live = drag?.clip.id === c.id ? drag : pending?.id === c.id ? pending : null;
                // Связанный с перетаскиваемым: тот же сдвиг (перенос) или те же границы (обрезка).
                const lead = !live && c.link ? (drag && drag.clip.link === c.link ? drag : pending ? view.clips.find((x) => x.id === pending.id && x.link === c.link) && pending : null) : null;
                const leadClip = lead ? view.clips.find((x) => x.id === ("clip" in lead ? lead.clip.id : lead.id)) : null;
                const start = live ? live.start : lead && leadClip ? ("mode" in lead && lead.mode !== "move" ? lead.start : c.start + (lead.start - leadClip.start)) : c.start;
                const duration = live ? live.duration : lead ? lead.duration : c.duration;
                const m = c.src ? byPath.get(c.src) : undefined;
                const label = c.kind === "caption" ? c.text : c.kind === "solid" ? c.text : (m?.name ?? c.src?.split("/").pop());
                const bg = m?.thumb ? `url("${convertFileSrc(m.thumb)}")` : undefined;
                return (
                  <div
                    key={c.id}
                    data-clip={c.id}
                    className={`clip ${tr.kind} ${selected.includes(c.id) ? "sel" : ""}`}
                    style={{ left: HEAD + start * zoom, width: Math.max(4, duration * zoom), backgroundImage: bg, zIndex: 1 + (order.get(c.id) ?? 0) }}
                    onPointerDown={(e) => onClipDown(e, c, "move")}
                    title={`${label ?? ""} · ${timecode(start, fps)} – ${timecode(start + duration, fps)}`}
                  >
                    <span className="h l" onPointerDown={(e) => onClipDown(e, c, "trimL")} />
                    {c.link && <Link2 className="lnk" size={11} strokeWidth={2} />}
                    {!!c.effects?.length && <Sparkles className="lnk fxmark" size={11} strokeWidth={2} />}
                    <span className="cl">{label}</span>
                    <span className="h r" onPointerDown={(e) => onClipDown(e, c, "trimR")} />
                  </div>
                );
              })}
            {ghost?.track === tr.id && (
              <div className={`clip ghost ${tr.kind}`} style={{ left: HEAD + ghost.start * zoom, width: Math.max(4, ghost.duration * zoom) }}>
                <span className="cl">{mediaDrag?.item.name}</span>
              </div>
            )}
          </div>
        ))}
      </>
    ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [view.tracks, view.clips, drag, pending, selected, zoom, visibleKey, byPath, order, ghost?.track, ghost?.start, ghost?.duration, t],
  );

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
        {lanes}
        <div className="playhead" style={{ left: HEAD + frame * zoom }} />
      </div>
      {view.clips.length === 0 && <div className="tl-empty">{t("edit.empty")}</div>}
    </div>
  );
}
