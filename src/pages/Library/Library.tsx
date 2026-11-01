import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Archive, Check, Copy, FolderInput, FolderOpen, Lock, MinusCircle, Package, Play, Plus, Save, Search, Square, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type LibraryRow, type LibraryView, commands as ipc, events } from "@/shared/ipc/bindings";
import { playSound } from "@/pages/Edit/EffectsColumn";
import { PresetPreview, TargetIcons } from "@/shared/ui/PresetPreview";
import { ControlPanel, defaultsOf, loc } from "@/shared/ui/ControlPanel";

/** «Библиотека» и большой каталог на «Монтаже»: паки слева (главный, базовый, свои), элементы карточками;
 *  предпросмотр, правка, копия, порядок перетаскиванием, перенос на пак слева — добавить в его конец,
 *  архивы. Эту же библиотеку использует Claude. */
export function Library({ onError, onToast, onUse, canApply }: { onError: (e: AppError) => void; onToast: (text: string) => void; onUse?: (row: LibraryRow) => void; canApply?: boolean }) {
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
  const [playing, setPlaying] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [drag, setDrag] = useState<{ row: LibraryRow; x: number; y: number; pack: string | null; before: string | null; copy: boolean } | null>(null);
  const dragRef = useRef(drag);
  dragRef.current = drag;

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
      .sort((a, b) => {
        // В паке — порядок пользователя: заданный, затем свои элементы, добавленные — в конец.
        if (current) {
          const place = (r: LibraryRow) => {
            const i = current.order?.indexOf(r.item.id) ?? -1;
            if (i >= 0) return i;
            const ref = current.refs?.indexOf(r.item.id) ?? -1;
            return ref >= 0 ? 200000 + ref : 100000;
          };
          const d = place(a) - place(b);
          if (d) return d;
        }
        return a.item.kind.localeCompare(b.item.kind) || loc(a.item.name, lang).localeCompare(loc(b.item.name, lang));
      });
  }, [lib, pack, current, query, lang]);
  const row = lib?.items.find((r) => r.item.id === sel) ?? null;
  const owner = packs.find((p) => p.id === row?.pack);

  useEffect(() => setValues(row ? defaultsOf(row.item.controls) : {}), [row?.item.id, row?.item.revision]);
  useEffect(() => setName(row ? loc(row.item.name, lang) : ""), [row?.item.id, row?.item.revision, lang]);
  const canOrder = !!current && !current.readonly && !query;

  /** Перетаскивание карточки: внутри пака — порядок, на пак слева — добавить в его конец (копия ссылкой,
   *  элемент один). Базовый пак принимать не может. */
  const beginDrag = (r: LibraryRow, e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const [x0, y0] = [e.clientX, e.clientY];
    let active = false;
    const noNative = (ev: Event) => ev.preventDefault();
    const move = (ev: PointerEvent) => {
      if (!active && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
      active = true;
      const under = document.elementFromPoint(ev.clientX, ev.clientY);
      const target = under?.closest<HTMLElement>("[data-pack]")?.dataset.pack ?? null;
      const accepts = !!target && !packs.find((p) => p.id === target)?.readonly && target !== r.pack && !packs.find((p) => p.id === target)?.refs?.includes(r.item.id);
      const card = canOrder ? under?.closest<HTMLElement>("[data-item]") : null;
      let before: string | null = null;
      if (card?.dataset.item && card.dataset.item !== r.item.id) {
        const q = card.getBoundingClientRect();
        const list = rows.map((x) => x.item.id);
        const i = list.indexOf(card.dataset.item);
        before = ev.clientX < q.left + q.width / 2 ? (list[i] ?? null) : (list[i + 1] ?? "$end");
      }
      document.body.classList.toggle("dragcopy", accepts);
      document.body.classList.toggle("dragno", !!target && !accepts);
      setDrag({ row: r, x: ev.clientX, y: ev.clientY, pack: accepts ? target : null, before, copy: accepts });
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", stop);
      window.removeEventListener("dragstart", noNative);
      document.body.classList.remove("dragcopy", "dragno");
      setDrag(null);
    };
    const up = () => {
      const d = dragRef.current;
      stop();
      if (!active || !d) return;
      if (d.pack) {
        void ipc.packItem(d.pack, r.item.id, true).then((x) => {
          done(x);
          if (x.status === "ok") onToast(t("library.addedTo", { pack: loc(packs.find((p) => p.id === d.pack)?.name, lang) }));
        });
      } else if (d.before && current) {
        const list = rows.map((x) => x.item.id).filter((id) => id !== r.item.id);
        const at = d.before === "$end" ? list.length : list.indexOf(d.before);
        list.splice(at < 0 ? list.length : at, 0, r.item.id);
        void ipc.packReorder(current.id, list).then(done);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", stop);
    window.addEventListener("dragstart", noNative);
  };

  /** Сохранить в элементе имя и значения панели как значения по умолчанию. */
  const saveEdits = async () => {
    if (!row) return;
    const controls = (Array.isArray(row.item.controls) ? row.item.controls : []).map((c: any) => (c.id && c.id in values ? { ...c, default: values[c.id] } : c));
    const old = row.item.name;
    const renamed = typeof old === "object" && old ? { ...old, [lang]: name.trim() || loc(old, lang) } : name.trim() || old;
    const r = await ipc.librarySave({ ...row.item, name: renamed, controls }, row.pack);
    if (r.status === "error") return onError(r.error);
    onToast(t("library.saved"));
  };

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
          <button key={p.id} data-pack={p.id} className={`libpack ${pack === p.id ? "on" : ""} ${drag?.pack === p.id ? "dropok" : ""}`} onClick={() => setPack(p.id)}>
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
            <div
              key={r.item.id}
              data-item={r.item.id}
              className={`libcard ${sel === r.item.id ? "on" : ""} ${drag?.row.item.id === r.item.id ? "lifted" : ""} ${drag?.before === r.item.id ? "insert" : ""}`}
              onPointerDown={(e) => beginDrag(r, e)}
              onClick={() => setSel(r.item.id)}
              onDoubleClick={() => onUse?.(r)}
            >
              <PresetPreview item={r.item} />
              <span className="libtop">
                <b>{loc(r.item.name, lang)}</b>
                <TargetIcons targets={(r.item.targets ?? [])} />
              </span>
              <span className="faint libdesc">{loc(r.item.description, lang)}</span>
              <span className="faint libmeta">
                {t(`library.kind.${r.item.kind}`)} · {t(`library.author.${r.item.author}`, { defaultValue: r.item.author })}
                {r.uses > 0 ? ` · ${t("library.uses", { count: r.uses })}` : ""}
              </span>
              {r.item.kind === "sound" && (
                <button
                  className="iconbtn libplay"
                  title={t("library.listen")}
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation();
                    playSound(r.item.id, setPlaying, onError, playing);
                  }}
                >
                  {playing === r.item.id ? <Square size={13} strokeWidth={2} /> : <Play size={13} strokeWidth={2} />}
                </button>
              )}
            </div>
          ))}
          {!rows.length && <p className="faint ihint">{t("library.nothing")}</p>}
        </div>
      </section>

      <aside className="libdetail">
        {row ? (
          <>
            <div className="libtop">
              {owner?.readonly ? <b>{loc(row.item.name, lang)}</b> : <input className="field" value={name} onChange={(e) => setName(e.target.value)} />}
              <TargetIcons targets={row.item.targets ?? []} />
            </div>
            <p className="faint">{loc(row.item.description, lang)}</p>
            <PresetPreview item={row.item} values={values} live />
            {row.item.kind === "sound" && (
              <button className="btn" onClick={() => playSound(row.item.id, setPlaying, onError, playing)}>
                {playing === row.item.id ? <Square size={14} strokeWidth={1.75} /> : <Play size={14} strokeWidth={1.75} />}
                {t("library.listen")}
              </button>
            )}
            <ControlPanel controls={row.item.controls} values={values} onChange={(id, v) => setValues((x) => ({ ...x, [id]: v }))} />
            {row.item.kind === "style" && <pre className="librules">{loc(row.item.rules, lang)}</pre>}
            <p className="faint libmeta">
              {t("library.inPack", { pack: loc(owner?.name, lang) })} · {t("library.revision", { n: row.item.revision })}
            </p>
            {onUse && (
              <button className="btn pri" disabled={row.item.kind === "effect" && !canApply} title={row.item.kind === "effect" && !canApply ? t("library.selectClip") : undefined} onClick={() => onUse(row)}>
                <Play size={14} strokeWidth={1.75} />
                {t(`library.use.${row.item.kind}`)}
              </button>
            )}
            <div className="ibtns">
              {!owner?.readonly && (
                <button className="btn" onClick={() => void saveEdits()}>
                  <Save size={14} strokeWidth={1.75} />
                  {t("library.save")}
                </button>
              )}
              {current && !current.readonly && current.id !== row.pack && (
                <button className="btn" onClick={() => void ipc.packItem(current.id, row.item.id, false).then(done)}>
                  <MinusCircle size={14} strokeWidth={1.75} />
                  {t("library.removeFromPack")}
                </button>
              )}
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
      {drag && (
        <div className="dragghost" style={{ left: drag.x + 12, top: drag.y + 12 }}>
          <span className="ic">
            {drag.copy && <Plus size={13} strokeWidth={2} />}
            {loc(drag.row.item.name, lang)}
          </span>
        </div>
      )}
    </div>
  );
}
