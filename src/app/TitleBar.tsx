import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Command, MenuItem } from "./commands";

export type MenuSpec = { id: string; label: string; items: MenuItem[] };

/** Пункты меню из реестра команд: название, иконка, клавиши; недоступные — серые. */
export function MenuList({ items, commands, onDone }: { items: MenuItem[]; commands: Map<string, Command>; onDone: () => void }) {
  const { t } = useTranslation();
  return (
    <>
      {items.map((id, i) => {
        if (id === "-") return <div key={`sep-${i}`} className="sep" />;
        const c = commands.get(id);
        if (!c) return null;
        const Icon = c.icon;
        return (
          <button
            key={id}
            className="mi"
            disabled={!(c.enabled?.() ?? true)}
            onClick={() => {
              onDone();
              c.run();
            }}
          >
            {Icon ? <Icon size={14} strokeWidth={1.75} /> : <span style={{ width: 14 }} />}
            {t(c.label)}
            {c.keys && <span className="k">{c.keys}</span>}
          </button>
        );
      })}
    </>
  );
}

/** Первая строка: знак, стандартные меню, заголовок, кнопки окна. Пустые места — перетаскивание. */
export function TitleBar({ menus, commands }: { menus: MenuSpec[]; commands: Map<string, Command> }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState<string | null>(null);
  const win = getCurrentWindow();

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [open]);

  return (
    <div className="sys">
      <span className="mark" data-tauri-drag-region />
      <div className="menubar">
        {menus.map((m) => (
          <div key={m.id} className={open === m.id ? "open" : ""} onMouseDown={(e) => e.stopPropagation()}>
            <button onClick={() => setOpen(open === m.id ? null : m.id)} onMouseEnter={() => open && setOpen(m.id)}>
              {t(m.label)}
            </button>
            {open === m.id && (
              <div className="menu" style={{ top: 30, left: 0 }}>
                <MenuList items={m.items} commands={commands} onDone={() => setOpen(null)} />
              </div>
            )}
          </div>
        ))}
      </div>
      <div className="drag" data-tauri-drag-region />
      <span className="wtitle">{t("app.title")}</span>
      <div className="wctl">
        <button aria-label="minimize" onClick={() => void win.minimize()}>
          <Minus size={14} strokeWidth={1.75} />
        </button>
        <button aria-label="maximize" onClick={() => void win.toggleMaximize()}>
          <Square size={12} strokeWidth={1.75} />
        </button>
        <button aria-label="close" className="close" onClick={() => void win.close()}>
          <X size={15} strokeWidth={1.75} />
        </button>
      </div>
    </div>
  );
}
