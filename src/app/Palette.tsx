import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Command } from "./commands";

/** Палитра команд (Ctrl+Shift+P): поиск по названию, стрелки, Enter. */
export function Palette({ commands, onClose }: { commands: Command[]; onClose: () => void }) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return commands.filter((c) => c.id !== "palette" && (c.enabled?.() ?? true) && (!q || t(c.label).toLowerCase().includes(q)));
  }, [commands, query, t]);

  const run = (c: Command | undefined) => {
    if (!c) return;
    onClose();
    c.run();
  };

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          autoFocus
          placeholder={t("palette.placeholder")}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            if (e.key === "ArrowDown") setIndex((i) => Math.min(i + 1, list.length - 1));
            if (e.key === "ArrowUp") setIndex((i) => Math.max(i - 1, 0));
            if (e.key === "Enter") run(list[index]);
          }}
        />
        <div className="list">
          {list.length === 0 && <div className="mi muted">{t("palette.empty")}</div>}
          {list.map((c, i) => {
            const Icon = c.icon;
            return (
              <button key={c.id} className={`mi ${i === index ? "sel" : ""}`} onMouseEnter={() => setIndex(i)} onClick={() => run(c)}>
                {Icon ? <Icon size={14} strokeWidth={1.75} /> : <span style={{ width: 14 }} />}
                {t(c.label)}
                {c.keys && <span className="k">{c.keys}</span>}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
