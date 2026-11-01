import { useSyncExternalStore } from "react";
import type { MediaItem } from "@/shared/ipc/bindings";

// Перенос файла медиа на таймлайн своим перетаскиванием мышью: встроенное перетаскивание
// браузера в окне Windows не работает, пока окно принимает файлы из проводника.

type Drag = { item: MediaItem; x: number; y: number } | null;
let drag: Drag = null;
const subs = new Set<() => void>();
let onDrop: ((item: MediaItem, x: number, y: number) => void) | null = null;

function set(next: Drag) {
  drag = next;
  for (const s of subs) s();
}

export function useDragMedia(): Drag {
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => drag,
  );
}

/** Кто примет файл (таймлайн регистрирует себя). */
export function setDropTarget(fn: typeof onDrop) {
  onDrop = fn;
}

/** Начать перенос с элемента медиа: перетаскивание начинается после сдвига на 5 px. */
export function startMediaDrag(item: MediaItem, e: React.PointerEvent) {
  const x0 = e.clientX;
  const y0 = e.clientY;
  let active = false;
  const move = (ev: PointerEvent) => {
    if (!active && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 5) return;
    active = true;
    set({ item, x: ev.clientX, y: ev.clientY });
  };
  const up = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (active) onDrop?.(item, ev.clientX, ev.clientY);
    set(null);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}
