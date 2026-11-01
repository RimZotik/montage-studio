import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft, Download, LayoutGrid, Loader, Play, Search, Square, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type LibraryRow, type LibraryView, type ProjectView, type SpeechModel, commands as ipc, events } from "@/shared/ipc/bindings";
import { Library } from "@/pages/Library/Library";
import { ControlPanel, defaultsOf, loc } from "@/shared/ui/ControlPanel";
import { PresetPreview, TargetIcons } from "@/shared/ui/PresetPreview";
import { startDrag } from "./dragMedia";

const FILTERS = ["all", "tool", "video", "audio", "text", "transition", "sound", "style"] as const;
type Filter = (typeof FILTERS)[number];

let player: HTMLAudioElement | null = null;
/** Послушать звук библиотеки; повторный вызов с тем же звуком — остановить. Одновременно один. */
export function playSound(id: string, onState: (playing: string | null) => void, onError: (e: AppError) => void, current: string | null) {
  player?.pause();
  player = null;
  if (current === id) return onState(null);
  void ipc.soundPreview(id).then((r) => {
    if (r.status === "error") return onError(r.error);
    const a = new Audio(convertFileSrc(r.data));
    player = a;
    a.onended = () => onState(null);
    void a.play();
    onState(id);
  });
}

/** Вкладка «Эффекты»: каталог библиотеки на «Монтаже» строками. Один щелчок — послушать звук или
 *  открыть карточку; двойной — применить (звук — от головки, эффект — на выделенные клипы); перенос —
 *  на таймлайн. Большой каталог с паками и карточками открывается отдельным окном. */
export function EffectsColumn({ view, selected, frame, onError, style }: { view: ProjectView; selected: string[]; frame: number; onError: (e: AppError) => void; style?: React.CSSProperties }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const fps = view.project.sequence.fps;
  const [lib, setLib] = useState<LibraryView | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [open, setOpen] = useState<LibraryRow | null>(null);
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [model, setModel] = useState<SpeechModel | null>(null);
  const [needModel, setNeedModel] = useState(false);
  const [ghost, setGhost] = useState<{ name: string; x: number; y: number } | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const [catalog, setCatalog] = useState(false);
  const clickTimer = useRef(0);

  const load = useCallback(() => void ipc.libraryView().then((r) => (r.status === "ok" ? setLib(r.data) : onError(r.error))), [onError]);
  useEffect(() => {
    load();
    void ipc.speechModel().then((r) => r.status === "ok" && setModel(r.data));
    const off = events.libraryChanged.listen(load);
    const offSpeech = events.speechProgress.listen(({ payload }) => setProgress(payload.done ? null : payload.progress));
    const offComp = events.componentProgress.listen(({ payload }) => {
      if (payload.id !== "speech") return;
      setProgress(payload.phase === "download" ? payload.received / Math.max(1, payload.total) : null);
      if (payload.phase === "done") {
        setNeedModel(false);
        setBusy(null);
        void ipc.speechModel().then((r) => r.status === "ok" && setModel(r.data));
      }
      if (payload.phase === "error") setBusy(null);
    });
    return () => {
      void off.then((f) => f());
      void offSpeech.then((f) => f());
      void offComp.then((f) => f());
      window.clearTimeout(clickTimer.current);
      player?.pause();
    };
  }, [load]);

  const rows = useMemo(() => {
    const q = query.toLowerCase();
    return (lib?.items ?? [])
      .filter((r) => filter === "all" || (["tool", "sound", "style"].includes(filter) ? r.item.kind === filter : (r.item.kind === "effect" && (r.item.targets ?? []).includes(filter)) || (filter === "audio" && r.item.kind === "tool" && (r.item.targets ?? []).includes("audio"))))
      .filter((r) => !q || `${loc(r.item.name, lang)} ${loc(r.item.description, lang)} ${(r.item.tags ?? []).join(" ")}`.toLowerCase().includes(q))
      .sort((a, b) => b.uses - a.uses || loc(a.item.name, lang).localeCompare(loc(b.item.name, lang)));
  }, [lib, query, filter, lang]);

  const applyEffect = useCallback(
    async (row: LibraryRow, clips: string[], params: Record<string, unknown> = {}) => {
      if (!clips.length) return setNote(t("library.selectClip"));
      setNote(null);
      const r = await ipc.effectApply(row.item.id, params, clips);
      if (r.status === "error") onError(r.error);
    },
    [onError, t],
  );

  /** Звук встает так, чтобы начаться в кадре `at` (ядро ставит по точке удара). */
  const placeSound = useCallback(
    (row: LibraryRow, at: number) => {
      const hit = Math.round((row.item.hitAt ?? 0) * fps);
      void ipc.soundPlace(row.item.id, at + hit, null).then((x) => (x.status === "error" ? onError(x.error) : setNote(t("library.soundAdded"))));
    },
    [fps, onError, t],
  );

  const applyStyle = useCallback((row: LibraryRow) => void ipc.styleApply(row.item.id, lang).then((x) => (x.status === "error" ? onError(x.error) : setNote(t("library.styleApplied")))), [lang, onError, t]);

  /** Двойной щелчок и кнопка в большом каталоге: то же действие элемента. */
  const useItem = useCallback(
    (row: LibraryRow) => {
      if (row.item.kind === "sound") placeSound(row, frame);
      else if (row.item.kind === "effect") void applyEffect(row, selected);
      else if (row.item.kind === "style") applyStyle(row);
      else showCard(row);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [applyEffect, applyStyle, frame, placeSound, selected],
  );

  const showCard = (row: LibraryRow) => {
    setOpen(row);
    setValues(defaultsOf(row.item.controls));
    setNote(null);
    setNeedModel(false);
  };

  /** Перетаскивание своим способом: встроенное в окне, принимающем файлы, не работает. */
  const beginDrag = (row: LibraryRow, e: React.PointerEvent) => {
    if (e.button !== 0) return;
    if (row.item.kind === "sound") {
      startDrag({ name: loc(row.item.name, lang), category: "sfx", seconds: row.item.seconds ?? 1, hasAudio: true, sound: { id: row.item.id, hitAt: row.item.hitAt ?? 0 } }, e);
      return;
    }
    if (row.item.kind !== "effect") return;
    const [x0, y0] = [e.clientX, e.clientY];
    let active = false;
    let over: HTMLElement | null = null;
    const hover = (el: HTMLElement | null) => {
      if (over !== el) {
        over?.classList.remove("fxdrop");
        el?.classList.add("fxdrop");
        over = el;
      }
    };
    const move = (ev: PointerEvent) => {
      if (!active && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 5) return;
      active = true;
      setGhost({ name: loc(row.item.name, lang), x: ev.clientX, y: ev.clientY });
      hover(document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-clip]") ?? null);
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
    };
    // Перетаскивание прервано (окно потеряло мышь): ничего не накладывать.
    const cancel = () => {
      stop();
      hover(null);
      setGhost(null);
    };
    const up = () => {
      stop();
      const id = over?.dataset.clip;
      hover(null);
      setGhost(null);
      if (active && id) void applyEffect(row, [id]);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
  };

  const onClickRow = (r: LibraryRow) => {
    if (r.item.kind === "sound") return playSound(r.item.id, setPlaying, onError, playing);
    // Карточка заменяет список: ждем, не будет ли второго щелчка.
    window.clearTimeout(clickTimer.current);
    clickTimer.current = window.setTimeout(() => showCard(r), 230);
  };

  const run = async () => {
    if (!open) return;
    setBusy(open.item.id);
    setNote(null);
    const r = await ipc.toolRun(open.item.id, values, selected);
    setBusy(null);
    setProgress(null);
    if (r.status === "error") return onError(r.error);
    if (r.data.status === "needsSpeechModel") return setNeedModel(true);
    setNote(r.data.status === "done" ? t("library.toolDone", { count: r.data.changed }) : r.data.note);
  };

  useEffect(() => {
    if (!catalog) return;
    const key = (e: KeyboardEvent) => e.key === "Escape" && setCatalog(false);
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [catalog]);

  return (
    <aside className="col fxcol" style={style}>
      {!open && (
        <>
          <div className="fxbar">
            <label className="search fxsearch">
              <Search size={14} strokeWidth={1.75} />
              <input placeholder={t("library.search")} value={query} onChange={(e) => setQuery(e.target.value)} />
            </label>
            <button className="iconbtn" title={t("library.catalog")} onClick={() => setCatalog(true)}>
              <LayoutGrid size={15} strokeWidth={1.75} />
            </button>
          </div>
          <div className="fxfilters">
            {FILTERS.map((f) => (
              <button key={f} className={filter === f ? "on" : ""} onClick={() => setFilter(f)}>
                {t(`library.filter.${f}`)}
              </button>
            ))}
          </div>
          <div className="fxlist">
            {rows.map((r) => (
              <div
                key={r.item.id}
                className={`fxrow ${playing === r.item.id ? "on" : ""}`}
                title={loc(r.item.description, lang)}
                onPointerDown={(e) => beginDrag(r, e)}
                onClick={() => onClickRow(r)}
                onDoubleClick={() => {
                  window.clearTimeout(clickTimer.current);
                  useItem(r);
                }}
              >
                {r.item.kind === "sound" && (playing === r.item.id ? <Square size={11} strokeWidth={2} /> : <Play size={11} strokeWidth={2} />)}
                <span className="fxname">{loc(r.item.name, lang)}</span>
                <TargetIcons targets={r.item.targets ?? []} />
              </div>
            ))}
            {!rows.length && <p className="faint ihint">{t("library.nothing")}</p>}
          </div>
        </>
      )}
      {note && <div className="fxnote">{note}</div>}
      {open && (
        <div className="fxpanel">
          <div className="fxpanelhead">
            <button className="iconbtn" title={t("library.back")} onClick={() => setOpen(null)}>
              <ChevronLeft size={15} strokeWidth={1.75} />
            </button>
            <b>{loc(open.item.name, lang)}</b>
            <button className="iconbtn" title={t("library.back")} onClick={() => setOpen(null)}>
              <X size={14} strokeWidth={1.75} />
            </button>
          </div>
          <PresetPreview item={open.item} values={values} live />
          <p className="faint fxdesc">{loc(open.item.description, lang)}</p>
          <ControlPanel controls={open.item.controls} values={values} tracks={view.tracks} onChange={(id, v) => setValues((x) => ({ ...x, [id]: v }))} />
          {open.item.kind === "effect" ? (
            <button className="btn pri" disabled={!selected.length} title={selected.length ? undefined : t("library.selectClip")} onClick={() => void applyEffect(open, selected, values)}>
              <Play size={14} strokeWidth={1.75} />
              {t("library.applyToSelected")}
            </button>
          ) : open.item.kind === "style" ? (
            <button className="btn pri" onClick={() => applyStyle(open)}>
              <Play size={14} strokeWidth={1.75} />
              {t("library.applyStyle")}
            </button>
          ) : needModel ? (
            <div className="fxmodel">
              <p className="faint">{t("library.modelNeeded", { mb: model?.downloadMb ?? 548 })}</p>
              <button
                className="btn pri"
                disabled={busy === "speech"}
                onClick={() => {
                  setBusy("speech");
                  void ipc.componentInstall("speech").then((r) => r.status === "error" && (onError(r.error), setBusy(null)));
                }}
              >
                {busy === "speech" ? <Loader size={14} strokeWidth={1.75} className="spin" /> : <Download size={14} strokeWidth={1.75} />}
                {busy === "speech" && progress != null ? `${Math.round(progress * 100)} %` : t("library.modelInstall")}
              </button>
            </div>
          ) : (
            <button className="btn pri" disabled={!!busy} onClick={() => void run()}>
              {busy ? <Loader size={14} strokeWidth={1.75} className="spin" /> : <Play size={14} strokeWidth={1.75} />}
              {busy ? (progress != null ? `${Math.round(progress * 100)} %` : t("library.running")) : t("library.run")}
            </button>
          )}
        </div>
      )}
      {ghost && (
        <div className="dragghost" style={{ left: ghost.x + 12, top: ghost.y + 12 }}>
          {ghost.name}
        </div>
      )}
      {catalog && (
        <div
          className="overlay catoverlay"
          onPointerDown={(e) => {
            if (e.target === e.currentTarget) setCatalog(false);
          }}
        >
          <div className="catbox">
            <div className="cathead">
              <b>{t("library.catalog")}</b>
              <button className="iconbtn" title={t("library.close")} onClick={() => setCatalog(false)}>
                <X size={15} strokeWidth={1.75} />
              </button>
            </div>
            <Library
              onError={onError}
              onToast={setNote}
              onUse={(row) => {
                if (row.item.kind === "tool") {
                  setCatalog(false);
                  showCard(row);
                } else useItem(row);
              }}
              canApply={selected.length > 0}
            />
          </div>
        </div>
      )}
    </aside>
  );
}
