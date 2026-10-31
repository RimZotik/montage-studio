import { convertFileSrc } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { AudioLines, Clapperboard, Image, Mic, Music, Upload } from "lucide-react";
import { type CSSProperties, type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type MediaItem, commands as ipc, events } from "@/shared/ipc/bindings";

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

type Props = {
  projectPath: string;
  /** Меняется после импорта перетаскиванием в окно — перечитать. */
  reloadKey: number;
  selected: string | null;
  onSelect: (item: MediaItem) => void;
  onError: (e: AppError) => void;
  onImport: (files: string[]) => void;
  style?: CSSProperties;
};

/** Медиа проекта: импорт, категории, миниатюры и волны (строятся фоном), выбор для предпросмотра. */
export function MediaColumn({ projectPath, reloadKey, selected, onSelect, onError, onImport, style }: Props) {
  const { t } = useTranslation();
  const [items, setItems] = useState<MediaItem[]>([]);
  const [cat, setCat] = useState("footage");
  const [menu, setMenu] = useState<{ item: MediaItem; x: number; y: number } | null>(null);

  const reload = useCallback(async () => {
    const r = await ipc.mediaList();
    if (r.status === "ok") setItems(r.data);
    else onError(r.error);
  }, [onError]);

  useEffect(() => {
    void reload();
  }, [reload, projectPath, reloadKey]);

  // Фоновая подготовка (миниатюры, волны, длительность) закончилась — перечитать.
  useEffect(() => {
    const off = events.mediaChanged.listen(() => void reload());
    return () => void off.then((f) => f());
  }, [reload]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);

  const counts = useMemo(() => Object.fromEntries(CATS.map((c) => [c.id, items.filter((i) => i.category === c.id).length])), [items]);
  const shown = items.filter((i) => i.category === cat);

  return (
    <section className="col media" style={style}>
      <div className="colhead">{t("prepare.media")}</div>
      <button
        className="dropzone"
        onClick={() =>
          void open({ multiple: true, title: t("prepare.import") }).then((f) => {
            if (Array.isArray(f)) onImport(f);
            else if (typeof f === "string") onImport([f]);
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
            className={`mc ${selected === m.path ? "sel" : ""}`}
            title={m.path}
            onClick={() => onSelect(m)}
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
