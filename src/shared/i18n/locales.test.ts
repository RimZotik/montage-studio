import { describe, expect, it } from "vitest";
import en from "./locales/en.json";
import ru from "./locales/ru.json";

function keys(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([k, v]) => keys(v, prefix ? `${prefix}.${k}` : k));
}

describe("переводы", () => {
  it("одинаковые ключи на обоих языках", () => {
    expect(keys(en).sort()).toEqual(keys(ru).sort());
  });
});
