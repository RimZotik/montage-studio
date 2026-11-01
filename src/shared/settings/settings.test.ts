import { afterEach, describe, expect, it, vi } from "vitest";

const sent: Record<string, unknown>[] = [];
vi.mock("@/shared/ipc/bindings", () => ({
  commands: {
    settingsGet: async () => ({ ui: { "a.saved": 5 } }),
    settingsSetUi: async (v: Record<string, unknown>) => void sent.push(v),
  },
}));
vi.stubGlobal("window", { addEventListener: () => {} });
const browserStore: Record<string, string> = { "layout.edit": '{"media":300}', other: "1" };
Object.defineProperties(browserStore, {
  getItem: { value: (k: string) => browserStore[k] ?? null },
  removeItem: { value: (k: string) => delete browserStore[k] },
});
vi.stubGlobal("localStorage", browserStore);

const { getSetting, loadSettings, setSetting } = await import("./settings");

afterEach(() => vi.useRealTimers());

describe("настройки интерфейса", () => {
  it("читает сохраненное и забирает ширины из браузера один раз", async () => {
    await loadSettings();
    expect(getSetting("a.saved", 0)).toBe(5);
    expect(getSetting("нет.ключа", 7)).toBe(7);
    expect(getSetting("layout.edit", null)).toEqual({ media: 300 });
  });

  it("значение меняется сразу, запись уходит пачкой", () => {
    vi.useFakeTimers();
    sent.length = 0;
    setSetting("b.x", 1);
    setSetting("b.x", 2);
    setSetting("b.y", true);
    expect(getSetting("b.x", 0)).toBe(2);
    expect(sent).toEqual([]);
    vi.runAllTimers();
    expect(sent).toEqual([{ "b.x": 2, "b.y": true, "layout.edit": { media: 300 } }]);
  });

  it("null возвращает к умолчанию", () => {
    setSetting("b.x", null);
    expect(getSetting("b.x", 9)).toBe(9);
  });
});
