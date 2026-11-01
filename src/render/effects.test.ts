import { describe, expect, it } from "vitest";
import type { AppliedEffect } from "@/shared/ipc/bindings";
import { effectsAt, evalExpr, volumeAt } from "./effects";

const fx = (spec: unknown, params: unknown = {}, enabled = true) => ({ id: "fx1", effect: "t", name: "T", enabled, params, controls: [], spec }) as AppliedEffect;

describe("выражения", () => {
  it("значения панели и действия слева направо", () => {
    expect(evalExpr(2, {})).toBe(2);
    expect(evalExpr("$a", { a: 3 })).toBe(3);
    expect(evalExpr("-$a", { a: 3 })).toBe(-3);
    expect(evalExpr("$a*2+1", { a: 3 })).toBe(7);
    expect(evalExpr("$a*0.5", { a: 3 })).toBe(1.5);
    expect(evalExpr("$a*-1", { a: 3 })).toBe(-3);
    expect(evalExpr("-$a*0.7", { a: 10 })).toBeCloseTo(-7);
    expect(evalExpr("$a/0", { a: 3 }), "деление на ноль не ломает кадр").toBe(3);
    expect(evalExpr("$нет", {})).toBe(0);
  });
});

describe("эффекты", () => {
  const fadeIn = fx({ tracks: [{ prop: "opacity", anchor: "start", keys: [[0, 0], ["$d", 1]] }] }, { d: 1 });
  const fadeOut = fx({ tracks: [{ prop: "opacity", anchor: "end", keys: [["-$d", 1], [0, 0]] }] }, { d: 1 });

  it("от начала и от конца клипа", () => {
    expect(effectsAt([fadeIn], 0, 300, 30).opacity).toBe(0);
    expect(effectsAt([fadeIn], 15, 300, 30).opacity).toBeCloseTo(0.5);
    expect(effectsAt([fadeIn], 200, 300, 30).opacity).toBe(1);
    expect(effectsAt([fadeOut], 100, 300, 30).opacity).toBe(1);
    expect(effectsAt([fadeOut], 285, 300, 30).opacity).toBeCloseTo(0.5);
  });

  it("на всю длину, постоянное значение, волна", () => {
    const zoom = fx({ tracks: [{ prop: "scale", anchor: "span", keys: [[0, 1], [1, 2]] }] });
    expect(effectsAt([zoom], 150, 300, 30).scale).toBeCloseTo(1.5);
    expect(effectsAt([fx({ tracks: [{ prop: "blur", value: "$a" }] }, { a: 4 })], 0, 10, 30).filter).toBe("blur(4px)");
    const shake = fx({ tracks: [{ prop: "x", wave: { amp: 10, freq: 1 } }] });
    expect(effectsAt([shake], 0, 300, 4).x).toBeCloseTo(0);
    expect(effectsAt([shake], 1, 300, 4).x).toBeCloseTo(10);
  });

  it("складываются: множители умножаются, сдвиги суммируются", () => {
    const a = fx({ tracks: [{ prop: "scale", value: 2 }, { prop: "x", value: 5 }] });
    const b = fx({ tracks: [{ prop: "scale", value: 3 }, { prop: "x", value: 7 }] });
    const r = effectsAt([a, b], 0, 10, 30);
    expect([r.scale, r.x]).toEqual([6, 12]);
  });

  it("выключенный не действует; пусто — без изменений", () => {
    expect(effectsAt([fx({ tracks: [{ prop: "scale", value: 2 }] }, {}, false)], 0, 10, 30).scale).toBe(1);
    expect(effectsAt(undefined, 0, 10, 30)).toMatchObject({ opacity: 1, scale: 1, filter: "" });
  });

  it("цвет текста из панели", () => {
    expect(effectsAt([fx({ text: { color: "$c", background: "#000000" } }, { c: "#ff0000" })], 0, 10, 30)).toMatchObject({ color: "#ff0000", background: "#000000" });
  });

  it("оформление текста из панели", () => {
    expect(effectsAt([fx({ text: { size: "$s", stroke: 0, weight: 900, uppercase: "$u" } }, { s: 48, u: true })], 0, 10, 30)).toMatchObject({ size: 48, stroke: 0, weight: 900, uppercase: true });
  });

  it("огибающая громкости — плавно между ключами", () => {
    const keys: [number, number][] = [[100, 1], [110, 0.2]];
    expect(volumeAt(keys, 50)).toBe(1);
    expect(volumeAt(keys, 105)).toBeCloseTo(0.6);
    expect(volumeAt(keys, 900)).toBeCloseTo(0.2);
    expect(volumeAt([], 5)).toBe(1);
  });
});
