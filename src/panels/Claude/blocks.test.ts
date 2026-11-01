import { describe, expect, it } from "vitest";
import { parseDecision, splitBlocks } from "./blocks";

describe("карточки в ответе Claude", () => {
  it("вырезает предложение и вопрос, текст вокруг остается", () => {
    const text = 'Предлагаю так.\n```studio-proposal\n{"id":"p1","kind":"tool","title":"Нарезка","description":"По 2 с"}\n```\nИли:\n```studio-ask\n{"id":"q1","question":"Темп?","options":["Спокойный","Быстрый"]}\n```';
    const parts = splitBlocks(text);
    expect(parts.map((p) => p.type)).toEqual(["text", "proposal", "text", "ask"]);
    expect(parts[1]).toMatchObject({ value: { id: "p1", title: "Нарезка" } });
  });

  it("битый блок показывается текстом", () => {
    expect(splitBlocks("```studio-proposal\n{oops\n```")[0]?.type).toBe("text");
  });

  it("решения", () => {
    expect(parseDecision("[proposal p1: accepted]")).toEqual({ id: "p1", verdict: "accepted" });
    expect(parseDecision("[answer q1] Быстрый")).toEqual({ id: "q1", verdict: "Быстрый" });
    expect(parseDecision("обычный текст")).toBeNull();
  });
});
