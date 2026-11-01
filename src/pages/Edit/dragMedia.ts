import { useSyncExternalStore } from "react";
import type { MediaItem } from "@/shared/ipc/bindings";

// Перенос на таймлайн своим перетаскиванием мышью: встроенное перетаскивание браузера в окне Windows
// не работает, пока окно принимает файлы из проводника. Переносится файл медиа или звук библиотеки.

export type DragPayload = {
  name: string;
  category: string;
  /** Длина, секунд; `null` — картинка. */
  seconds: number | null;
  hasAudio: boolean;
  path?: string;
  sound?: { id: string; hitAt: number };
};

type Drag = { payload: DragPayload; x: number; y: number } | null;
let drag: Drag = null;
const subs = new Set<() => void>();
let onDrop: ((payload: DragPayload, x: number, y: number) => void) | null = null;

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

/** Кто примет перенос (таймлайн регистрирует себя). */
export function setDropTarget(fn: typeof onDrop) {
  onDrop = fn;
}

export function mediaPayload(item: MediaItem): DragPayload {
  return { name: item.name, category: item.category, seconds: item.category === "images" ? null : (item.probe?.durationS ?? 5), hasAudio: item.probe?.audio !== false, path: item.path };
}

/** Начать перенос: перетаскивание начинается после сдвига на 5 px. Встроенное перетаскивание картинки
 *  браузером запрещено: оно забирает мышь, и подстановка появлялась бы только при отпускании. */
export function startDrag(payload: DragPayload, e: React.PointerEvent) {
  const x0 = e.clientX;
  const y0 = e.clientY;
  let active = false;
  const noNative = (ev: Event) => ev.preventDefault();
  const move = (ev: PointerEvent) => {
    if (!active && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 5) return;
    active = true;
    document.body.classList.add("dragging");
    set({ payload, x: ev.clientX, y: ev.clientY });
  };
  const stop = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", stop);
    window.removeEventListener("dragstart", noNative);
    document.body.classList.remove("dragging");
    set(null);
  };
  const up = (ev: PointerEvent) => {
    if (active) onDrop?.(payload, ev.clientX, ev.clientY);
    stop();
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", stop);
  window.addEventListener("dragstart", noNative);
}
