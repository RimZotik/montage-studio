import { AudioLines, Blend, Clapperboard, Download, Loader, Play, Search, Type, Wand2, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type LibraryRow, type LibraryView, type ProjectView, type SpeechModel, commands as ipc, events } from "@/shared/ipc/bindings";
import { ControlPanel, defaultsOf, loc } from "@/shared/ui/ControlPanel";

export const TARGET_ICON: Record<string, LucideIcon> = { video: Clapperboard, audio: AudioLines, text: Type, sequence: Wand2, transition: Blend };
const FILTERS = ["all", "tool", "video", "audio", "text", "transition"] as const;
type Filter = (typeof FILTERS)[number];

/** Значки назначения в углу карточки. */
export function TargetIcons({ targets }: { targets: string[] }) {
  const { t } = useTranslation();
  return (
    <span className="fxtargets">
      {targets.map((x) => {
        const Icon = TARGET_ICON[x];
        return Icon ? (
          <span key={x} title={t(`library.target.${x}`)}>
            <Icon size={12} strokeWidth={1.75} />
          </span>
        ) : null;
      })}
    </span>
  );
}

/** Вкладка «Эффекты»: каталог библиотеки на «Монтаже». Эффект перетаскивается на клип или
 *  накладывается на выделение; инструмент открывает панель и запускается кнопкой. */
export function EffectsColumn({ view, selected, onError, style }: { view: ProjectView; selected: string[]; onError: (e: AppError) => void; style?: React.CSSProperties }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
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

  const load = useCallback(() => void ipc.libraryView().then((r) => r.status === "ok" ? setLib(r.data) : onError(r.error)), [onError]);
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
    };
  }, [load]);

  const rows = useMemo(() => {
    const q = query.toLowerCase();
    return (lib?.items ?? [])
      .filter((r) => filter === "all" || (filter === "tool" ? r.item.kind === "tool" : (r.item.targets ?? []).includes(filter)))
      .filter((r) => !q || `${loc(r.item.name, lang)} ${loc(r.item.description, lang)} ${(r.item.tags ?? []).join(" ")}`.toLowerCase().includes(q))
      .sort((a, b) => b.uses - a.uses || loc(a.item.name, lang).localeCompare(loc(b.item.name, lang)));
  }, [lib, query, filter, lang]);

  const applyTo = useCallback(
    async (row: LibraryRow, clips: string[]) => {
      if (!clips.length) return setNote(t("library.selectClip"));
      setNote(null);
      const r = await ipc.effectApply(row.item.id, {}, clips);
      if (r.status === "error") onError(r.error);
    },
    [onError, t],
  );

  /** Перетаскивание своим способом: встроенное в окне, принимающем файлы, не работает. */
  const startDrag = (row: LibraryRow, e: React.PointerEvent) => {
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
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const id = over?.dataset.clip;
      hover(null);
      setGhost(null);
      if (active && id) void applyTo(row, [id]);
      else if (!active) void applyTo(row, selected);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
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

  return (
    <aside className="col fxcol" style={style}>
      <label className="search fxsearch">
        <Search size={14} strokeWidth={1.75} />
        <input placeholder={t("library.search")} value={query} onChange={(e) => setQuery(e.target.value)} />
      </label>
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
            className={`fxcard ${open?.item.id === r.item.id ? "on" : ""}`}
            title={loc(r.item.description, lang)}
            onPointerDown={(e) => startDrag(r, e)}
            onClick={() => {
              if (r.item.kind !== "tool") return;
              setOpen(r);
              setValues(defaultsOf(r.item.controls));
              setNote(null);
              setNeedModel(false);
            }}
          >
            <span className="fxname">{loc(r.item.name, lang)}</span>
            <TargetIcons targets={(r.item.targets ?? [])} />
          </div>
        ))}
        {!rows.length && <p className="faint ihint">{t("library.nothing")}</p>}
      </div>
      {note && <div className="fxnote">{note}</div>}
      {open && (
        <div className="fxpanel">
          <div className="fxpanelhead">
            <b>{loc(open.item.name, lang)}</b>
            <button className="iconbtn" onClick={() => setOpen(null)}>
              <X size={14} strokeWidth={1.75} />
            </button>
          </div>
          <p className="faint fxdesc">{loc(open.item.description, lang)}</p>
          <ControlPanel controls={open.item.controls} values={values} tracks={view.tracks} onChange={(id, v) => setValues((x) => ({ ...x, [id]: v }))} />
          {needModel ? (
            <div className="fxmodel">
              <p className="faint">{t("library.modelNeeded", { mb: model?.downloadMb ?? 548 })}</p>
              <button className="btn pri" disabled={busy === "speech"} onClick={() => { setBusy("speech"); void ipc.componentInstall("speech").then((r) => r.status === "error" && (onError(r.error), setBusy(null))); }}>
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
    </aside>
  );
}
