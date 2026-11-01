import { describe, expect, it } from "vitest";
import { smartLoader, splitExports } from "./smart";

describe("сборка сцен", () => {
  it("несколько имен в одном export становятся отдельными", () => {
    expect(splitExports("export const A = 800, B = 128;\nconst c = 1;")).toBe("export const A = 800;\nexport const B = 128;\nconst c = 1;");
    expect(splitExports("export const A = 1, B = 2\nexport const C = 3")).toBe("export const A = 1;\nexport const B = 2\nexport const C = 3");
  });

  it("запятые в значениях, типах и строках не режутся", () => {
    for (const src of ["export const a: Map<string, number> = new Map();", 'export const s = "x, y = 1";', "export const f = (a: number, b = 2) => a + b;", "export const o = { a: 1, b: 2 };"]) {
      expect(splitExports(src)).toBe(src);
    }
  });

  it("модули находят друг друга, json и библиотеки", () => {
    const load = smartLoader(
      {
        "smart/lib/x.ts": "export const W = 800, H = 128;",
        "smart/d.json": '{"v":5}',
        "smart/Main.tsx": 'import { W, H } from "./lib/x"; import d from "./d.json"; import React from "react"; export const sum: number = W + H + d.v; export default () => <div />;',
      },
      (p) => p,
    );
    const m = load("smart/Main.tsx").exports;
    expect(m.sum).toBe(933);
    expect(typeof m.default).toBe("function");
  });

  it("короткий путь к файлу ведет в media/assets", () => {
    const load = smartLoader({ "smart/A.ts": 'import { staticFile } from "remotion"; export const a = staticFile("img/x.png"), b = staticFile("media/images/y.png");' }, (p) => `/${p}`);
    expect(load("smart/A.ts").exports).toMatchObject({ a: "/media/assets/img/x.png", b: "/media/images/y.png" });
  });

  it("чужие библиотеки недоступны", () => {
    const load = smartLoader({ "smart/A.ts": 'import fs from "fs"; export const a = fs;' }, (p) => p);
    expect(() => load("smart/A.ts")).toThrow(/not available/);
  });
});
