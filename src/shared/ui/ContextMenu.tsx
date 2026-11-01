import type { LucideIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

export type MenuItem = { label: string; icon?: LucideIcon; keys?: string; disabled?: boolean; danger?: boolean; run: () => void } | "-" | { title: string };

/** Контекстное меню у курсора: не выходит за окно, закрывается щелчком мимо или Esc. */
export function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (r) setPos({ left: Math.max(4, Math.min(x, window.innerWidth - r.width - 8)), top: Math.max(4, Math.min(y, window.innerHeight - r.height - 8)) });
  }, [x, y]);
  useEffect(() => {
    const down = (e: PointerEvent) => !ref.current?.contains(e.target as Node) && onClose();
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("keydown", key);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("keydown", key);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);
  return (
    <div ref={ref} className="menu ctxmenu" style={{ position: "fixed", ...pos }} onContextMenu={(e) => e.preventDefault()}>
      {items.map((m, i) => {
        if (m === "-") return <div key={i} className="msep" />;
        if ("title" in m) return <div key={i} className="mlabel">{m.title}</div>;
        const Icon = m.icon;
        return (
          <button
            key={i}
            className={`mi ctxitem ${m.danger ? "danger" : ""}`}
            disabled={m.disabled}
            onClick={() => {
              onClose();
              m.run();
            }}
          >
            <span className="ctxicon">{Icon && <Icon size={14} strokeWidth={1.75} />}</span>
            <span className="ctxlabel">{m.label}</span>
            {m.keys && <span className="k">{m.keys}</span>}
          </button>
        );
      })}
    </div>
  );
}
