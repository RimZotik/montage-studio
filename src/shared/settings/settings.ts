import { useCallback, useSyncExternalStore } from "react";
import { commands as ipc } from "@/shared/ipc/bindings";

// Настройки интерфейса (docs/settings.md): одно хранилище на всю программу. Новой настройке нужен
// только ключ: `const [on, setOn] = useSetting("monitor.safeZone", true)`. Хранит ядро в settings.json;
// здесь — копия, чтобы значение менялось сразу, запись идет пачкой и не на каждое движение ползунка.

const WRITE_DELAY_MS = 250;

let values: Record<string, unknown> = {};
let unsent: Record<string, unknown> = {};
let timer: ReturnType<typeof setTimeout> | null = null;
const listeners = new Set<() => void>();

function notify() {
  for (const l of listeners) l();
}

function flush() {
  if (timer) clearTimeout(timer);
  timer = null;
  const batch = unsent;
  unsent = {};
  if (Object.keys(batch).length) void ipc.settingsSetUi(batch);
}

/** Прочитать настройки до первого показа окна: панель сразу в том виде, как ее оставили. */
export async function loadSettings(): Promise<void> {
  const s = await ipc.settingsGet();
  values = (s.ui as Record<string, unknown> | undefined) ?? {};
  // Ширины колонок раньше лежали в браузере: забираем один раз.
  try {
    for (const key of Object.keys(localStorage)) {
      if (!key.startsWith("layout.")) continue;
      if (!(key in values)) setSetting(key, JSON.parse(localStorage.getItem(key) ?? "null"));
      localStorage.removeItem(key);
    }
  } catch {
    /* браузерного хранилища может не быть */
  }
  window.addEventListener("beforeunload", flush);
  notify();
}

export function getSetting<T>(key: string, fallback: T): T {
  return key in values ? (values[key] as T) : fallback;
}

/** Записать значение; `null` — вернуть к значению по умолчанию. */
export function setSetting(key: string, value: unknown) {
  const next = { ...values };
  if (value === null) delete next[key];
  else next[key] = value;
  values = next;
  unsent[key] = value;
  if (timer) clearTimeout(timer);
  timer = setTimeout(flush, WRITE_DELAY_MS);
  notify();
}

/** Сбросить все значения интерфейса. */
export function resetSettings() {
  for (const key of Object.keys(values)) setSetting(key, null);
}

/** Настройка как состояние компонента: помнится между запусками и едина во всех окнах программы. */
export function useSetting<T>(key: string, fallback: T): [T, (next: T | ((prev: T) => T)) => void] {
  const raw = useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => values[key],
  );
  const set = useCallback(
    (next: T | ((prev: T) => T)) => {
      const prev = getSetting(key, fallback);
      setSetting(key, typeof next === "function" ? (next as (p: T) => T)(prev) : next);
    },
    // Значение по умолчанию задается при объявлении и не меняется.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  );
  return [raw === undefined ? fallback : (raw as T), set];
}
