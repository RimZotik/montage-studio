import { describe, expect, it } from "vitest";
import { comboOf, findByKeys } from "./commands";

const ev = (key: string, code: string, mods: Partial<{ ctrlKey: boolean; shiftKey: boolean; altKey: boolean }> = {}) => ({ ctrlKey: false, shiftKey: false, altKey: false, key, code, ...mods });

describe("горячие клавиши", () => {
  it("не зависят от раскладки", () => {
    expect(comboOf(ev("т", "KeyN", { ctrlKey: true }))).toBe("Ctrl+N");
    expect(comboOf(ev("P", "KeyP", { ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+P");
    expect(comboOf(ev("ё", "Backquote", { ctrlKey: true }))).toBe("Ctrl+`");
    expect(comboOf(ev("F5", "F5"))).toBe("F5");
  });

  it("недоступная команда не срабатывает", () => {
    const list = [
      { id: "a", label: "a", keys: "Ctrl+N", enabled: () => false, run: () => {} },
      { id: "b", label: "b", keys: "Ctrl+O", run: () => {} },
    ];
    expect(findByKeys(list, "Ctrl+N")).toBeUndefined();
    expect(findByKeys(list, "Ctrl+O")?.id).toBe("b");
  });
});
