import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { AudioLines, Clapperboard, Image, Mic, Music, Upload } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type MediaItem, commands as ipc } from "@/shared/ipc/bindings";

const CATS: { id: string; icon: ReactNode }[] = [
  { id: "footage", icon: <Clapperboard size={14} strokeWidth={1.75} /> },
  { id: "voice", icon: <Mic size={14} strokeWidth={1.75} /> },
  { id: "music", icon: <Music size={14} strokeWidth={1.75} /> },
  { id: "sfx", icon: <AudioLines size={14} strokeWidth={1.75} /> },
  { id: "images", icon: <Image size={14} strokeWidth={1.75} /> },
];

function duration(s: number | null | undefined): string {
  if (s == null) return "";
  const m = Math.floor(s / 60);
  return `${m}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
}

/** Медиа проекта: перетаскивание файлов в окно, импорт, категории, миниатюры. */
export function MediaColumn({ projectPath, onError, onImported }: { projectPath: string; onError: (e: AppError) => void; onImported: (n: number) => void }) {
  const { t } = useTranslation();
  const [items, setItems] = useState<MediaItem[]>([]);
  const [cat, setCat] = useState("footage");
  const [over, setOver] = useState(false);
  const [menu, setMenu] = useState<{ item: MediaItem; x: number; y: number } | null>(null);

  const reload = useCallback(async () => {
    const r = await ipc.mediaList();
    if (r.status === "ok") setItems(r.data);
    else onError(r.error);
  }, [onError]);

  const importFiles = useCallback(
    async (files: string[]) => {
      if (!files.length) return;
      const r = await ipc.mediaImport(files);
      if (r.status === "error") return onError(r.error);
      onImported(r.data);
      await reload();
    },
    [onError, onImported, reload],
  );

  useEffect(() => {
    void reload();
  }, [reload, projectPath]);

  // Файлы, перетащенные в окно, — в проект.
  useEffect(() => {
    let off: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setOver(true);
        else if (e.payload.type === "leave") setOver(false);
        else if (e.payload.type === "drop") {
          setOver(false);
          void importFiles(e.payload.paths);
        }
      })
      .then((f) => (off = f))
      .catch(() => {});
    return () => off?.();
  }, [importFiles]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);

  const counts = useMemo(() => Object.fromEntries(CATS.map((c) => [c.id, items.filter((i) => i.category === c.id).length])), [items]);
  const shown = items.filter((i) => i.category === cat);

  return (
    <section className={`col media ${over ? "over" : ""}`}>
      <div className="colhead">{t("prepare.media")}</div>
      <button
        className="dropzone"
        onClick={() =>
          void open({ multiple: true, title: t("prepare.import") }).then((f) => {
            if (Array.isArray(f)) void importFiles(f);
            else if (typeof f === "string") void importFiles([f]);
          })
        }
      >
        <Upload size={18} strokeWidth={1.5} />
        <span>{t("prepare.drop")}</span>
        <small>{t("prepare.dropHint")}</small>
      </button>
      <div className="cats">
        {CATS.map((c) => (
          <button key={c.id} className={`cat ${cat === c.id ? "on" : ""}`} onClick={() => setCat(c.id)}>
            {c.icon}
            {t(`media.${c.id}`)}
            <span className="n">{counts[c.id] ?? 0}</span>
          </button>
        ))}
      </div>
      <div className="mgrid">
        {shown.map((m) => (
          <div
            key={m.path}
            className="mc"
            title={m.path}
            onDoubleClick={() => void revealItemInDir(`${projectPath}\\${m.path.replaceAll("/", "\\")}`)}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ item: m, x: e.clientX, y: e.clientY });
            }}
          >
            <div className={`mt ${m.category}`}>{m.thumb ? <img src={convertFileSrc(m.thumb)} alt="" /> : CATS.find((c) => c.id === m.category)?.icon}</div>
            <div className="mn">
              <span>{m.name}</span>
              <span className="mono">{duration(m.probe?.durationS)}</span>
            </div>
          </div>
        ))}
        {shown.length === 0 && <div className="faint mempty">{t("prepare.emptyCat")}</div>}
      </div>
      {menu && (
        <div className="menu" style={{ position: "fixed", left: menu.x, top: menu.y }} onMouseDown={(e) => e.stopPropagation()}>
          <div className="mlabel">{t("prepare.moveTo")}</div>
          {CATS.filter((c) => c.id !== menu.item.category).map((c) => (
            <button
              key={c.id}
              className="mi"
              onClick={() => {
                setMenu(null);
                void ipc.mediaMove(menu.item.path, c.id).then((r) => (r.status === "error" ? onError(r.error) : reload()));
              }}
            >
              {c.icon}
              {t(`media.${c.id}`)}
            </button>
          ))}
          <div className="sep" />
          <button
            className="mi"
            onClick={() => {
              setMenu(null);
              void revealItemInDir(`${projectPath}\\${menu.item.path.replaceAll("/", "\\")}`);
            }}
          >
            {t("home.reveal")}
          </button>
        </div>
      )}
    </section>
  );
}
