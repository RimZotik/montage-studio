import { Player } from "@remotion/player";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Archive, Check, Copy, FolderInput, FolderOpen, Lock, Package, Plus, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type Clip, type LibraryRow, type LibraryView, type Track, commands as ipc, events } from "@/shared/ipc/bindings";
import { TimelineComposition } from "@/render/TimelineComposition";
import { TargetIcons } from "@/pages/Edit/EffectsColumn";
import { ControlPanel, defaultsOf, loc } from "@/shared/ui/ControlPanel";

const FPS = 30;
const LEN = FPS * 3;

/** Маленький предпросмотр эффекта: заливка с текстом, та же композиция, что и в монтаже. */
function EffectPreview({ row, values }: { row: LibraryRow; values: Record<string, unknown> }) {
  const props = useMemo(() => {
    const fx = { id: "fx1", effect: row.item.id, name: row.item.name, enabled: true, params: values, controls: row.item.controls, spec: row.item.spec };
    const isText = (row.item.targets ?? []).includes("text") && !(row.item.targets ?? []).includes("video");
    const tracks: Track[] = [
      { id: "s1", kind: "captions", name: "S1", muted: false, locked: false, volume: 1 },
      { id: "v1", kind: "video", name: "V1", muted: false, locked: false, volume: 1 },
    ];
    const base = { offset: 0, link: null, src: null, props: { volume: 1, opacity: 1, scale: 1, x: 0, y: 0, fadeIn: 0, fadeOut: 0 }, start: 0, duration: LEN } as const;
    const clips: Clip[] = [
      { ...base, id: "a", track: "v1", kind: "solid", text: "#F28A4B", effects: isText ? [] : [fx] },
      { ...base, id: "b", track: "s1", kind: "caption", text: "Montage", effects: isText ? [fx] : [] },
    ] as Clip[];
    return { sequence: { width: 720, height: 405, fps: FPS, durationFrames: 0, background: "#1f1a17" }, tracks, clips, srcOf: (p: string) => p };
  }, [row, values]);
  return <Player component={TimelineComposition} inputProps={props} durationInFrames={LEN} compositionWidth={720} compositionHeight={405} fps={FPS} autoPlay loop style={{ width: "100%", borderRadius: 8 }} acknowledgeRemotionLicense />;
}

/** «Библиотека»: паки слева (главный, базовый, свои), элементы справа; панель, копия, состав паков,
 *  архивы. Эту же библиотеку использует Claude. */
export function Library({ onError, onToast }: { onError: (e: AppError) => void; onToast: (text: string) => void }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const [lib, setLib] = useState<LibraryView | null>(null);
  const [pack, setPack] = useState("all");
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [newName, setNewName] = useState<string | null>(null);
  // Удаление стирает и историю правок: второе нажатие подтверждает.
  const [confirmDelete, setConfirmDelete] = useState(false);
  useEffect(() => setConfirmDelete(false), [sel]);

  const load = useCallback(() => void ipc.libraryView().then((r) => (r.status === "ok" ? setLib(r.data) : onError(r.error))), [onError]);
  useEffect(() => {
    load();
    const off = events.libraryChanged.listen(load);
    return () => void off.then((f) => f());
  }, [load]);

  const packs = lib?.packs ?? [];
  const current = packs.find((p) => p.id === pack);
  const rows = useMemo(() => {
    const q = query.toLowerCase();
    return (lib?.items ?? [])
      .filter((r) => pack === "all" || r.pack === pack || current?.refs?.includes(r.item.id))
      .filter((r) => !q || `${r.item.id} ${loc(r.item.name, lang)} ${loc(r.item.description, lang)}`.toLowerCase().includes(q))
      .sort((a, b) => a.item.kind.localeCompare(b.item.kind) || loc(a.item.name, lang).localeCompare(loc(b.item.name, lang)));
  }, [lib, pack, current, query, lang]);
  const row = lib?.items.find((r) => r.item.id === sel) ?? null;
  const owner = packs.find((p) => p.id === row?.pack);

  useEffect(() => setValues(row ? defaultsOf(row.item.controls) : {}), [row?.item.id, row?.item.revision]);

  const done = (r: { status: "ok" } | { status: "error"; error: AppError }) => r.status === "error" && onError(r.error);

  const copy = async () => {
    if (!row) return;
    const taken = new Set(lib?.items.map((r) => r.item.id));
    let id = `${row.item.id}-copy`;
    for (let n = 2; taken.has(id); n++) id = `${row.item.id}-copy-${n}`;
    const controls = (Array.isArray(row.item.controls) ? row.item.controls : []).map((c: any) => (c.id && c.id in values ? { ...c, default: values[c.id] } : c));
    const r = await ipc.librarySave({ ...row.item, id, controls, author: "user", revision: 1 }, null);
    if (r.status === "error") return onError(r.error);
    setSel(id);
    setPack("all");
    onToast(t("library.copied"));
  };

  const exportPack = async () => {
    if (!current) return;
    const path = await saveDialog({ defaultPath: `${loc(current.name, lang) || current.id}.zip`, filters: [{ name: "Zip", extensions: ["zip"] }] });
    if (!path) return;
    const r = await ipc.packExport(current.id, path);
    if (r.status === "ok") onToast(t("library.exported", { count: r.data }));
    else onError(r.error);
  };

  const importPack = async () => {
    const path = await openDialog({ filters: [{ name: "Zip", extensions: ["zip"] }] });
    if (typeof path !== "string") return;
    const r = await ipc.packImport(path);
    if (r.status === "ok") setPack(r.data.id);
    else onError(r.error);
  };

  const createPack = async () => {
    if (!newName?.trim()) return setNewName(null);
    const r = await ipc.packNew(newName.trim());
    setNewName(null);
    if (r.status === "ok") setPack(r.data.id);
    else onError(r.error);
  };

  return (
    <div className="libpage">
      <aside className="libpacks">
        <div className="colhead">{t("library.packs")}</div>
        <button className={`libpack ${pack === "all" ? "on" : ""}`} onClick={() => setPack("all")}>
          <Package size={14} strokeWidth={1.75} />
          <span>{t("library.allItems")}</span>
          <em>{lib?.items.length ?? 0}</em>
        </button>
        {packs.map((p) => (
          <button key={p.id} className={`libpack ${pack === p.id ? "on" : ""}`} onClick={() => setPack(p.id)}>
            {p.readonly ? <Lock size={14} strokeWidth={1.75} /> : <Package size={14} strokeWidth={1.75} />}
            <span>{loc(p.name, lang)}</span>
            <em>{(lib?.items.filter((r) => r.pack === p.id).length ?? 0) + (p.refs?.length ?? 0)}</em>
          </button>
        ))}
        {newName != null ? (
          <input className="field" autoFocus placeholder={t("library.packName")} value={newName} onChange={(e) => setNewName(e.target.value)} onBlur={() => void createPack()} onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()} />
        ) : (
          <button className="btn quiet" onClick={() => setNewName("")}>
            <Plus size={14} strokeWidth={1.75} />
            {t("library.newPack")}
          </button>
        )}
        <span className="end" />
        <button className="btn quiet" onClick={() => void importPack()}>
          <FolderInput size={14} strokeWidth={1.75} />
          {t("library.import")}
        </button>
        {lib && (
          <button className="btn quiet" onClick={() => void revealItemInDir(`${lib.root}\\packs`)}>
            <FolderOpen size={14} strokeWidth={1.75} />
            {t("library.openFolder")}
          </button>
        )}
      </aside>

      <section className="libmain">
        <div className="libbar">
          <label className="search">
            <Search size={14} strokeWidth={1.75} />
            <input placeholder={t("library.search")} value={query} onChange={(e) => setQuery(e.target.value)} />
          </label>
          <span className="end" />
          {current && (
            <button className="btn" onClick={() => void exportPack()}>
              <Archive size={14} strokeWidth={1.75} />
              {t("library.export")}
            </button>
          )}
          {current && !current.readonly && current.id !== "main" && (
            <button className="btn" onClick={() => void ipc.packRemove(current.id).then((r) => { done(r); setPack("all"); })}>
              <Trash2 size={14} strokeWidth={1.75} />
              {t("library.deletePack")}
            </button>
          )}
        </div>
        <div className="libgrid">
          {rows.map((r) => (
            <button key={r.item.id} className={`libcard ${sel === r.item.id ? "on" : ""}`} onClick={() => setSel(r.item.id)}>
              <span className="libtop">
                <b>{loc(r.item.name, lang)}</b>
                <TargetIcons targets={(r.item.targets ?? [])} />
              </span>
              <span className="faint libdesc">{loc(r.item.description, lang)}</span>
              <span className="faint libmeta">
                {t(`library.kind.${r.item.kind}`)} · {t(`library.author.${r.item.author}`, { defaultValue: r.item.author })}
                {r.uses > 0 ? ` · ${t("library.uses", { count: r.uses })}` : ""}
              </span>
            </button>
          ))}
          {!rows.length && <p className="faint ihint">{t("library.nothing")}</p>}
        </div>
      </section>

      <aside className="libdetail">
        {row ? (
          <>
            <div className="libtop">
              <b>{loc(row.item.name, lang)}</b>
              <TargetIcons targets={(row.item.targets ?? [])} />
            </div>
            <p className="faint">{loc(row.item.description, lang)}</p>
            {row.item.kind === "effect" && <EffectPreview row={row} values={values} />}
            <ControlPanel controls={row.item.controls} values={values} onChange={(id, v) => setValues((x) => ({ ...x, [id]: v }))} />
            {row.item.kind === "style" && <pre className="librules">{loc(row.item.rules, lang)}</pre>}
            <p className="faint libmeta">
              {t("library.inPack", { pack: loc(owner?.name, lang) })} · {t("library.revision", { n: row.item.revision })}
            </p>
            <div className="ibtns">
              <button className="btn" onClick={() => void copy()} title={t("library.copyHint")}>
                <Copy size={14} strokeWidth={1.75} />
                {t("library.copy")}
              </button>
              {!owner?.readonly && (
                <button className={`btn ${confirmDelete ? "danger" : ""}`} onClick={() => (confirmDelete ? void ipc.libraryDelete(row.item.id).then((r) => { done(r); setSel(null); }) : setConfirmDelete(true))}>
                  <Trash2 size={14} strokeWidth={1.75} />
                  {confirmDelete ? t("library.deleteSure") : t("edit.delete")}
                </button>
              )}
            </div>
            <div className="ctlsec">{t("library.addToPack")}</div>
            {packs.filter((p) => !p.readonly && p.id !== row.pack).map((p) => {
              const inside = !!p.refs?.includes(row.item.id);
              return (
                <button key={p.id} className="libpack" onClick={() => void ipc.packItem(p.id, row.item.id, !inside).then(done)}>
                  {inside ? <Check size={14} strokeWidth={2} /> : <Plus size={14} strokeWidth={1.75} />}
                  <span>{loc(p.name, lang)}</span>
                </button>
              );
            })}
          </>
        ) : (
          <p className="faint ihint">{t("library.pick")}</p>
        )}
      </aside>
    </div>
  );
}
