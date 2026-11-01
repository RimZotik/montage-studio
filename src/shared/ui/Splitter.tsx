import { useCallback, useRef, useState } from "react";
import { useSetting } from "@/shared/settings/settings";

/** Ширины колонок с запоминанием (у каждой раскладки свой ключ): хранит сервис настроек. */
export function useWidths<K extends string>(key: string, defaults: Record<K, number>) {
  const [saved, save] = useSetting<Partial<Record<K, number>>>(`layout.${key}`, {});
  const set = useCallback((name: K, value: number) => save((all) => ({ ...all, [name]: value })), [save]);
  return [{ ...defaults, ...saved } as Record<K, number>, set] as const;
}

/** Граница между колонками: тянуть мышью, двойной щелчок — ширина по умолчанию. `side` — с какой
 *  стороны от границы колонка, чью ширину меняем; `vertical` — граница по горизонтали (высота
 *  панели под ней, `side: "right"` — панель снизу). */
export function Splitter({ width, min, max, side, vertical, onChange, onReset }: { width: number; min: number; max: number; side: "left" | "right"; vertical?: boolean; onChange: (w: number) => void; onReset: () => void }) {
  const start = useRef<{ x: number; w: number } | null>(null);
  const [active, setActive] = useState(false);
  return (
    <div
      className={`splitter ${vertical ? "v" : ""} ${active ? "on" : ""}`}
      onDoubleClick={onReset}
      onPointerDown={(e) => {
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
        start.current = { x: vertical ? e.clientY : e.clientX, w: width };
        setActive(true);
      }}
      onPointerMove={(e) => {
        if (!start.current) return;
        const dx = (vertical ? e.clientY : e.clientX) - start.current.x;
        onChange(Math.round(Math.min(max, Math.max(min, start.current.w + (side === "left" ? dx : -dx)))));
      }}
      onPointerUp={() => {
        start.current = null;
        setActive(false);
      }}
    />
  );
}
