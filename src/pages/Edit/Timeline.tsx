import { convertFileSrc } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { AlignVerticalJustifyEnd, AlignVerticalJustifyStart, ArrowLeftToLine, Bookmark, Captions, CheckCheck, Copy, Eye, EyeOff, FileDown, FileUp, Link2, Lock, LockOpen, Magnet, Maximize2, MousePointer2, Plus, Scissors, SplitSquareHorizontal, Sparkles, Trash2, Type, Unlink, Volume2, VolumeX, ZoomIn, ZoomOut } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type Clip, type MediaItem, type Op, type ProjectView, type Track, commands as ipc } from "@/shared/ipc/bindings";
import { ContextMenu, type MenuItem } from "@/shared/ui/ContextMenu";
import { type DragPayload, setDropTarget, useDragMedia } from "./dragMedia";
import { snap, timecode } from "./time";

export type EditTool = "select" | "razor";

export type TimelineActions = {
  split: () => void;
  remove: (ripple: boolean) => void;
  marker: (frame?: number) => void;
  text: (frame?: number) => void;
  duplicate: () => void;
  selectAll: () => void;
};

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
  tool: EditTool;
  onTool: (t: EditTool) => void;
  snapOn: boolean;
  onSnap: (on: boolean) => void;
  actions: TimelineActions;
  onOpenEffects: () => void;
  onNote: (text: string) => void;
};

type Drag = { mode: "move" | "trimL" | "trimR"; clip: Clip; ids: Set<string>; x0: number; track: string; start: number; duration: number; offset: number; moved: boolean; collapse: string[] | null };
type Place = { track: string; start: number; duration: number };
type Box = { x0: number; y0: number; x: number; y: number; base: string[]; moved: boolean };
type Menu = { x: number; y: number; clip?: Clip; track?: Track; frame: number };

const HEAD = 132;
const SNAP_PX = 8;

/** Дорожка подходит для переносимого: картинка и видео — на видео, звук — на звук (видео можно и на
 *  звуковую дорожку — возьмется звук). */
function fits(track: Track, p: DragPayload): boolean {
  if (track.kind === "video") return p.category === "footage" || p.category === "images";
  if (track.kind === "audio") return p.category !== "images";
  return false;
}

/** Таймлайн: дорожки, клипы, линейка, головка, панель инструментов, рамка выделения, контекстные меню;
 *  перенос (и группы), обрезка краев, привязка, подстановка файлов и звуков. Правки уходят пакетами
 *  операций в ядро (одна отмена на действие). */
export function Timeline({ view, frame, total, zoom, onZoom, onSeek, selected, onSelect, apply, media, tool, onTool, snapOn, onSnap, actions, onOpenEffects, onNote }: Props) {
  const { t } = useTranslation();
  const fps = view.project.sequence.fps;
  const scroller = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [box, setBox] = useState<Box | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [razor, setRazor] = useState<number | null>(null);
  // Отпущенная правка до ответа ядра: клипы остаются там, где их отпустили (без рывка назад).
  const [pending, setPending] = useState<Map<string, Place> | null>(null);
  useEffect(() => setPending(null), [view.clips]);
  const mediaDrag = useDragMedia();
  const byPath = useMemo(() => new Map(media.map((m) => [m.path, m])), [media]);
  const order = useMemo(() => new Map(view.clips.map((c, i) => [c.id, i])), [view.clips]);
  const trackOf = useMemo(() => new Map(view.tracks.map((tr) => [tr.id, tr])), [view.tracks]);
  const width = Math.max(total + fps * 10, fps * 40) * zoom;
  const closeMenu = useCallback(() => setMenu(null), []);

  const withLinks = useCallback(
    (ids: string[]) => {
      const links = new Set(view.clips.filter((c) => c.link && ids.includes(c.id)).map((c) => c.link));
      return view.clips.filter((c) => ids.includes(c.id) || (c.link && links.has(c.link))).map((c) => c.id);
    },
    [view.clips],
  );

  const frameAt = useCallback(
    (clientX: number) => {
      const el = scroller.current;
      if (!el) return 0;
      const r = el.getBoundingClientRect();
      return Math.max(0, Math.round((clientX - r.left - HEAD + el.scrollLeft) / zoom));
    },
    [zoom],
  );

  const snapPoints = useCallback(
    (except: Set<string> | null) => (snapOn ? [0, frame, ...view.markers.map((m) => m.frame), ...view.clips.filter((c) => !except?.has(c.id)).flatMap((c) => [c.start, c.start + c.duration])] : []),
    [frame, view.clips, view.markers, snapOn],
  );

  /** Куда ляжет перенос под курсором: подходящая дорожка под ним или ближайшая подходящая; у видео со
   *  звуком — сразу и звуковая часть. Длина — настоящая. */
  const dropAt = useCallback(
    (p: DragPayload, x: number, y: number): Place[] => {
      const lane = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-track]");
      const over = view.tracks.findIndex((tr) => tr.id === lane?.dataset.track);
      if (over < 0) return [];
      const usable = view.tracks.map((tr, i) => ({ tr, i })).filter(({ tr }) => fits(tr, p) && !tr.locked).sort((a, b) => Math.abs(a.i - over) - Math.abs(b.i - over));
      const target = usable[0]?.tr;
      if (!target) return [];
      const duration = p.seconds == null ? fps * 3 : Math.max(1, Math.round(p.seconds * fps));
      const start = snap(frameAt(x), snapPoints(null), SNAP_PX / zoom);
      const out = [{ track: target.id, start, duration }];
      if (target.kind === "video" && p.category === "footage" && p.hasAudio) {
        const audio = view.tracks.filter((tr) => tr.kind === "audio" && !tr.locked);
        const free = audio.find((tr) => !view.clips.some((c) => c.track === tr.id && c.start < start + duration && c.start + c.duration > start)) ?? audio[0];
        if (free) out.push({ track: free.id, start, duration });
      }
      return out;
    },
    [view.tracks, view.clips, fps, frameAt, snapPoints, zoom],
  );

  // Отпущено над дорожкой — новый клип; мимо дорожки — ничего.
  useEffect(() => {
    setDropTarget((p, x, y) => {
      const [main, audio] = dropAt(p, x, y);
      if (!main) return;
      if (p.sound) {
        void ipc.soundPlace(p.sound.id, main.start + Math.round(p.sound.hitAt * fps), main.track).then((r) => r.status === "error" && onNote(r.error.message));
        return;
      }
      if (audio) void apply([{ op: "addLinked", videoTrack: main.track, audioTrack: audio.track, src: p.path ?? "", start: main.start, duration: main.duration, offset: null }], "add");
      else void apply([{ op: "addClip", track: main.track, kind: "media", src: p.path ?? null, text: null, start: main.start, duration: main.duration, offset: null }], "add");
    });
    return () => setDropTarget(null);
  }, [dropAt, apply, fps, onNote]);
  const ghosts = mediaDrag ? dropAt(mediaDrag.payload, mediaDrag.x, mediaDrag.y) : [];

  const onClipDown = (e: React.PointerEvent, clip: Clip, mode: Drag["mode"]) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    if (tool === "razor") {
      const at = frameAt(e.clientX);
      if (at > clip.start && at < clip.start + clip.duration && !trackOf.get(clip.track)?.locked) void apply([{ op: "splitClip", id: clip.id, at }], "split");
      return;
    }
    const group = withLinks([clip.id]);
    const had = selected.includes(clip.id);
    if (e.shiftKey || e.ctrlKey) {
      onSelect(had ? selected.filter((id) => !group.includes(id)) : [...selected, ...group]);
      if (had) return;
    } else if (!had) onSelect(group);
    if (trackOf.get(clip.track)?.locked) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const moving = mode === "move" ? new Set(withLinks(had || e.shiftKey || e.ctrlKey ? [...selected, ...group] : group)) : new Set(group);
    setDrag({ mode, clip, ids: moving, x0: e.clientX, track: clip.track, start: clip.start, duration: clip.duration, offset: clip.offset ?? 0, moved: false, collapse: had && !e.shiftKey && !e.ctrlKey ? group : null });
  };

  const selectInBox = (b: Box) => {
    const el = scroller.current;
    if (!el) return;
    const [l, r] = [Math.min(b.x0, b.x), Math.max(b.x0, b.x)];
    const [top, bottom] = [Math.min(b.y0, b.y), Math.max(b.y0, b.y)];
    const hit: string[] = [];
    el.querySelectorAll<HTMLElement>("[data-clip]").forEach((c) => {
      const q = c.getBoundingClientRect();
      if (q.left < r && q.right > l && q.top < bottom && q.bottom > top && c.dataset.clip) hit.push(c.dataset.clip);
    });
    onSelect(withLinks([...b.base, ...hit]));
  };

  const onMove = (e: React.PointerEvent) => {
    if (tool === "razor") setRazor(frameAt(e.clientX));
    if (box) {
      const moved = box.moved || Math.hypot(e.clientX - box.x0, e.clientY - box.y0) > 3;
      const next = { ...box, x: e.clientX, y: e.clientY, moved };
      setBox(next);
      if (moved) selectInBox(next);
      const el = scroller.current;
      if (el) {
        const r = el.getBoundingClientRect();
        if (e.clientX > r.right - 24) el.scrollLeft += 16;
        else if (e.clientX < r.left + HEAD + 24) el.scrollLeft -= 16;
      }
      return;
    }
    if (!drag) return;
    const df = Math.round((e.clientX - drag.x0) / zoom);
    const c = drag.clip;
    const pts = snapPoints(drag.ids);
    const radius = SNAP_PX / zoom;
    const source = c.src ? byPath.get(c.src)?.probe?.durationS : null;
    // Длина файла ограничивает растяжение клипа вправо (у текста и заливки — без ограничения).
    const maxDur = c.kind === "media" && source ? Math.round(source * fps) - (c.offset ?? 0) : Infinity;
    // Пока мышь почти не сдвинулась — клип на месте (без рывка к ближней точке привязки).
    if (!drag.moved && Math.abs(e.clientX - drag.x0) <= 3 && drag.mode === "move") return;
    let next = { ...drag, moved: true };
    if (drag.mode === "move") {
      // Вся группа едет вместе и не уходит левее нуля.
      const room = Math.min(...view.clips.filter((x) => drag.ids.has(x.id)).map((x) => x.start));
      const lowest = c.start - room;
      let start = Math.max(lowest, c.start + df);
      const end = snap(start + c.duration, pts, radius);
      start = end !== start + c.duration ? end - c.duration : snap(start, pts, radius);
      const lane = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-track]");
      const over = view.tracks.find((tr) => tr.id === lane?.dataset.track);
      const fromKind = trackOf.get(c.track)?.kind;
      // На другую дорожку едет один клип со связанным; группа остается на своих.
      const single = withLinks([c.id]).length === drag.ids.size;
      next = { ...next, start: Math.max(lowest, start), track: single && over && over.kind === fromKind && !over.locked ? over.id : drag.track };
    } else if (drag.mode === "trimL") {
      const minStart = Math.max(0, c.start - (c.offset ?? 0));
      const start = Math.min(Math.max(snap(c.start + df, pts, radius), c.kind === "media" ? minStart : 0), c.start + c.duration - 1);
      next = { ...next, start, duration: c.duration + (c.start - start), offset: Math.max(0, (c.offset ?? 0) + (start - c.start)) };
    } else {
      const end = snap(c.start + c.duration + df, pts, radius);
      next = { ...next, duration: Math.min(Math.max(1, end - c.start), maxDur) };
    }
    setDrag(next);
  };

  const previews = useMemo(() => {
    if (pending) return pending;
    const m = new Map<string, Place>();
    if (!drag?.moved) return m;
    const delta = drag.start - drag.clip.start;
    for (const c of view.clips) {
      if (!drag.ids.has(c.id)) continue;
      if (drag.mode === "move") m.set(c.id, { track: c.id === drag.clip.id ? drag.track : c.track, start: c.start + delta, duration: c.duration });
      else m.set(c.id, { track: c.track, start: drag.start, duration: drag.duration });
    }
    return m;
  }, [drag, pending, view.clips]);

  const onUp = () => {
    if (box) {
      if (!box.moved) onSelect(box.base);
      setBox(null);
      return;
    }
    if (!drag) return;
    const d = drag;
    setDrag(null);
    if (!d.moved) {
      if (d.collapse) onSelect(d.collapse);
      return;
    }
    const c = d.clip;
    setPending(new Map(previews));
    const done = (ok: boolean) => !ok && setPending(null);
    if (d.mode === "move") {
      const delta = d.start - c.start;
      if (!delta && d.track === c.track) return setPending(null);
      // Связанные двигает ядро: по одной операции на связку, ведущий клип первым.
      const seen = new Set<string>();
      const ops: Op[] = [];
      for (const x of [c, ...view.clips.filter((y) => d.ids.has(y.id) && y.id !== c.id)]) {
        if (x.link && seen.has(x.link)) continue;
        if (x.link) seen.add(x.link);
        ops.push({ op: "moveClip", id: x.id, track: x.id === c.id && d.track !== c.track ? d.track : null, start: x.start + delta });
      }
      void apply(ops, "move").then(done);
    } else {
      void apply([{ op: "trimClip", id: c.id, start: d.start, duration: d.duration, offset: c.kind === "media" ? d.offset : null }], "trim").then(done);
    }
  };

  const onLaneDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || e.target !== e.currentTarget || tool !== "select") return;
    scroller.current?.setPointerCapture(e.pointerId);
    setBox({ x0: e.clientX, y0: e.clientY, x: e.clientX, y: e.clientY, base: e.shiftKey || e.ctrlKey ? selected : [], moved: false });
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

  const zoomBy = (k: number) => onZoom(Math.min(40, Math.max(0.05, zoom * k)));
  const fit = () => {
    const el = scroller.current;
    if (!el) return;
    onZoom(Math.min(40, Math.max(0.05, (el.clientWidth - HEAD - 40) / Math.max(1, total))));
    el.scrollLeft = 0;
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

  // Наложение субтитров видно сразу: верхний — в верхней половине дорожки, нижний — в нижней.
  const captionRow = useMemo(() => {
    const out = new Map<string, "up" | "down">();
    const ids = new Set(view.tracks.filter((tr) => tr.kind === "captions").map((tr) => tr.id));
    const caps = view.clips.filter((c) => ids.has(c.track));
    for (const c of caps) {
      const crossing = caps.filter((x) => x.id !== c.id && x.track === c.track && x.start < c.start + c.duration && x.start + x.duration > c.start);
      if (!crossing.length) continue;
      out.set(c.id, crossing.some((x) => (order.get(x.id) ?? 0) < (order.get(c.id) ?? 0)) ? "up" : "down");
    }
    return out;
  }, [view.clips, view.tracks, order]);

  const openMenu = (e: React.MouseEvent, next: Omit<Menu, "x" | "y" | "frame">) => {
    e.preventDefault();
    e.stopPropagation();
    if (next.clip && !selected.includes(next.clip.id)) onSelect(withLinks([next.clip.id]));
    setMenu({ ...next, x: e.clientX, y: e.clientY, frame: frameAt(e.clientX) });
  };

  const ghostsKey = ghosts.map((g) => `${g.track}:${g.start}:${g.duration}`).join("|");

  // Клипы не перерисовываются от движения головки: при воспроизведении меняется только она.
  const [cullFrom, cullTo] = [Math.floor(fromF / (fps * 5)) * fps * 5, (Math.floor(toF / (fps * 5)) + 1) * fps * 5];
  const lanes = useMemo(
    () => (
      <>
        {view.tracks.map((tr) => (
          <div key={tr.id} className={`lane ${tr.kind} ${tr.muted ? "muted" : ""} ${tr.locked ? "locked" : ""}`} data-track={tr.id} onPointerDown={onLaneDown} onContextMenu={(e) => openMenu(e, { track: tr })}>
            <div className="lhead" onContextMenu={(e) => openMenu(e, { track: tr })}>
              <b className={`swatch ${tr.kind}`} />
              <span className="lname">{tr.name}</span>
              <span className="lkind">{tr.kind === "captions" ? t(`edit.captionMode.${tr.captionMode ?? "burned"}Short`) : t(`edit.track.${tr.kind}`)}</span>
              <button className="iconbtn" title={t("edit.mute")} onClick={() => void apply([{ op: "setTrack", id: tr.id, muted: !tr.muted, locked: null, volume: null, captionMode: null }], "track")}>
                {tr.kind === "audio" ? tr.muted ? <VolumeX size={12} strokeWidth={1.75} /> : <Volume2 size={12} strokeWidth={1.75} /> : tr.muted ? <EyeOff size={12} strokeWidth={1.75} /> : <Eye size={12} strokeWidth={1.75} />}
              </button>
              <button className="iconbtn" title={t("edit.lock")} onClick={() => void apply([{ op: "setTrack", id: tr.id, muted: null, locked: !tr.locked, volume: null, captionMode: null }], "track")}>
                {tr.locked ? <Lock size={12} strokeWidth={1.75} /> : <LockOpen size={12} strokeWidth={1.75} />}
              </button>
            </div>
            {view.clips
              .filter((c) => (previews.get(c.id)?.track ?? c.track) === tr.id)
              // Вне видимого участка клип не рисуется (кроме того, что сейчас тянут).
              .filter((c) => (c.start + c.duration >= cullFrom && c.start <= cullTo) || previews.has(c.id) || selected.includes(c.id))
              .map((c) => {
                const live = previews.get(c.id);
                const start = live?.start ?? c.start;
                const duration = live?.duration ?? c.duration;
                const m = c.src ? byPath.get(c.src) : undefined;
                const label = c.kind === "caption" ? c.text : c.kind === "solid" ? c.text : (m?.name ?? c.src?.split("/").pop());
                const bg = m?.thumb ? `url("${convertFileSrc(m.thumb)}")` : undefined;
                const row = captionRow.get(c.id);
                return (
                  <div
                    key={c.id}
                    data-clip={c.id}
                    className={`clip ${tr.kind} ${selected.includes(c.id) ? "sel" : ""} ${row ?? ""}`}
                    style={{ left: HEAD + start * zoom, width: Math.max(4, duration * zoom), backgroundImage: bg, zIndex: 1 + (order.get(c.id) ?? 0) }}
                    onPointerDown={(e) => onClipDown(e, c, "move")}
                    onContextMenu={(e) => openMenu(e, { clip: c, track: tr })}
                    title={`${label ?? ""} · ${timecode(start, fps)} – ${timecode(start + duration, fps)}`}
                  >
                    <span className="h l" onPointerDown={(e) => onClipDown(e, c, "trimL")} />
                    {c.link && <Link2 className="lnk" size={11} strokeWidth={2} />}
                    {!!c.effects?.length && <Sparkles className="lnk fxmark" size={11} strokeWidth={2} />}
                    {duration * zoom > 34 && <span className="cl">{label}</span>}
                    <span className="h r" onPointerDown={(e) => onClipDown(e, c, "trimR")} />
                  </div>
                );
              })}
            {ghosts
              .filter((g) => g.track === tr.id)
              .map((g) => (
                <div key={g.track} className={`clip ghost ${tr.kind}`} style={{ left: HEAD + g.start * zoom, width: Math.max(4, g.duration * zoom), zIndex: 100000 }}>
                  <span className="cl">{mediaDrag?.payload.name}</span>
                </div>
              ))}
          </div>
        ))}
      </>
    ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [view.tracks, view.clips, previews, selected, zoom, visibleKey, byPath, order, ghostsKey, t, tool, captionRow, drag?.moved],
  );

  const scrub = (e: React.PointerEvent) => {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    onSeek(frameAt(e.clientX));
  };

  const hasSel = selected.length > 0;
  const trackOps = (tr: Track): MenuItem[] => {
    const empty = !view.clips.some((c) => c.track === tr.id);
    const sameKind = view.tracks.filter((x) => x.kind === tr.kind).length;
    return [
      { title: `${t("edit.tools.track")} ${tr.name}` },
      ...(tr.kind === "captions"
        ? ([
            { label: t("edit.captionMode.burned"), icon: (tr.captionMode ?? "burned") === "burned" ? CheckCheck : Type, run: () => void apply([{ op: "setTrack", id: tr.id, muted: null, locked: null, volume: null, captionMode: "burned" }], "track") },
            { label: t("edit.captionMode.file"), icon: tr.captionMode === "file" ? CheckCheck : Captions, run: () => void apply([{ op: "setTrack", id: tr.id, muted: null, locked: null, volume: null, captionMode: "file" }], "track") },
            {
              label: t("edit.captionMode.import"),
              icon: FileUp,
              run: () =>
                void openDialog({ filters: [{ name: "Subtitles", extensions: ["srt", "vtt", "ass"] }] }).then((p) => {
                  if (typeof p === "string") void ipc.captionsImport(p).then((r) => onNote(r.status === "ok" ? t("edit.captionMode.imported", { count: r.data }) : r.error.message));
                }),
            },
            { label: t("edit.captionMode.save"), icon: FileDown, run: () => void ipc.captionsSave().then((r) => onNote(r.status === "ok" ? t("edit.captionMode.saved") : r.error.message)) },
            "-",
          ] as MenuItem[])
        : []),
      { label: t("edit.tools.addVideoTrack"), icon: Plus, run: () => void apply([{ op: "addTrack", kind: "video" }], "track") },
      { label: t("edit.tools.addAudioTrack"), icon: Plus, run: () => void apply([{ op: "addTrack", kind: "audio" }], "track") },
      { label: t("edit.tools.removeTrack"), icon: Trash2, disabled: !empty || sameKind < 2 || tr.kind === "captions", run: () => void apply([{ op: "removeTrack", id: tr.id }], "track") },
    ];
  };

  const menuItems = (m: Menu): MenuItem[] => {
    if (m.clip) {
      const ids = selected.includes(m.clip.id) ? selected : withLinks([m.clip.id]);
      const linked = view.clips.some((c) => ids.includes(c.id) && c.link);
      const inside = frame > m.clip.start && frame < m.clip.start + m.clip.duration;
      return [
        { label: t("edit.tools.split"), icon: SplitSquareHorizontal, keys: "S", disabled: !inside, run: actions.split },
        { label: t("edit.tools.duplicate"), icon: Copy, keys: "Ctrl+D", run: actions.duplicate },
        { label: t("edit.tools.effects"), icon: Sparkles, run: onOpenEffects },
        "-",
        { label: t("edit.tools.front"), icon: AlignVerticalJustifyStart, run: () => void apply([{ op: "arrangeClips", ids, top: true }], "order") },
        { label: t("edit.tools.back"), icon: AlignVerticalJustifyEnd, run: () => void apply([{ op: "arrangeClips", ids, top: false }], "order") },
        linked ? { label: t("edit.tools.unlink"), icon: Unlink, run: () => void apply([{ op: "setLink", ids, linked: false }], "link") } : { label: t("edit.tools.link"), icon: Link2, disabled: ids.length < 2, run: () => void apply([{ op: "setLink", ids, linked: true }], "link") },
        "-",
        { label: t("edit.tools.delete"), icon: Trash2, keys: "Delete", danger: true, run: () => actions.remove(false) },
        { label: t("edit.tools.ripple"), icon: ArrowLeftToLine, keys: "Shift+Delete", danger: true, run: () => actions.remove(true) },
      ];
    }
    return [
      { label: t("edit.tools.addText"), icon: Type, keys: "T", run: () => actions.text(m.frame) },
      { label: t("edit.tools.marker"), icon: Bookmark, keys: "M", run: () => actions.marker(m.frame) },
      { label: t("edit.tools.selectAll"), icon: CheckCheck, keys: "Ctrl+A", run: actions.selectAll },
      "-",
      ...(m.track ? trackOps(m.track) : []),
    ];
  };

  const tip = (key: string, keys?: string) => (keys ? `${t(`edit.tools.${key}`)} (${keys})` : t(`edit.tools.${key}`));
  const scrollX = scroller.current?.scrollLeft ?? 0;
  const rect = scroller.current?.getBoundingClientRect();
  const boxLeft = box && rect ? Math.max(scrollX + HEAD, Math.min(box.x0, box.x) - rect.left + scrollX) : 0;
  const boxRight = box && rect ? Math.max(scrollX + HEAD, Math.max(box.x0, box.x) - rect.left + scrollX) : 0;

  return (
    <div className="tlwrap">
      <div className="tltools">
        <div className="tlgroup">
          <button className={`iconbtn ${tool === "select" ? "on" : ""}`} title={tip("select", "V")} onClick={() => onTool("select")}>
            <MousePointer2 size={15} strokeWidth={1.75} />
          </button>
          <button className={`iconbtn ${tool === "razor" ? "on" : ""}`} title={tip("razor", "C")} onClick={() => onTool("razor")}>
            <Scissors size={15} strokeWidth={1.75} />
          </button>
        </div>
        <i className="tlsep" />
        <button className="iconbtn" title={tip("split", "S")} onClick={actions.split}>
          <SplitSquareHorizontal size={15} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={tip("duplicate", "Ctrl+D")} disabled={!hasSel} onClick={actions.duplicate}>
          <Copy size={15} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={tip("delete", "Delete")} disabled={!hasSel} onClick={() => actions.remove(false)}>
          <Trash2 size={15} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={tip("ripple", "Shift+Delete")} disabled={!hasSel} onClick={() => actions.remove(true)}>
          <ArrowLeftToLine size={15} strokeWidth={1.75} />
        </button>
        <i className="tlsep" />
        <button className="iconbtn" title={tip("addText", "T")} onClick={() => actions.text()}>
          <Type size={15} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={tip("marker", "M")} onClick={() => actions.marker()}>
          <Bookmark size={15} strokeWidth={1.75} />
        </button>
        <button className={`iconbtn ${snapOn ? "on" : ""}`} title={tip("snap", "N")} onClick={() => onSnap(!snapOn)}>
          <Magnet size={15} strokeWidth={1.75} />
        </button>
        <span style={{ flex: 1 }} />
        <button className="iconbtn" title={tip("zoomOut", "-")} onClick={() => zoomBy(1 / 1.25)}>
          <ZoomOut size={15} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={tip("fit")} onClick={fit}>
          <Maximize2 size={14} strokeWidth={1.75} />
        </button>
        <button className="iconbtn" title={tip("zoomIn", "+")} onClick={() => zoomBy(1.25)}>
          <ZoomIn size={15} strokeWidth={1.75} />
        </button>
      </div>
      <div className={`timeline ${tool === "razor" ? "razor" : ""}`} ref={scroller} onWheel={onWheel} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp} onPointerLeave={() => setRazor(null)}>
        <div className="tl-inner" style={{ width: width + HEAD }}>
          <div className="tl-ruler" onPointerDown={scrub} onPointerMove={(e) => e.buttons === 1 && !drag && !box && onSeek(frameAt(e.clientX))}>
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
          {frame * zoom >= view0.left - 1 && <div className="playhead" style={{ left: HEAD + frame * zoom }} />}
          {tool === "razor" && razor != null && razor * zoom >= view0.left && <div className="razorline" style={{ left: HEAD + razor * zoom }} />}
          {box?.moved && rect && (
            <div className="tlbox" style={{ left: boxLeft, width: boxRight - boxLeft, top: Math.min(box.y0, box.y) - rect.top + (scroller.current?.scrollTop ?? 0), height: Math.abs(box.y - box.y0) }} />
          )}
        </div>
        {view.clips.length === 0 && <div className="tl-empty">{t("edit.empty")}</div>}
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu)} onClose={closeMenu} />}
    </div>
  );
}
