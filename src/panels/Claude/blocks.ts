// Карточки в ответах Claude: блоки ```studio-proposal``` и ```studio-ask``` (docs/assistant-layer.md)
// вырезаются из текста и показываются карточками; решение уходит обратно служебной строкой.

export type Proposal = { id: string; kind: string; title: string; description?: string; details?: string[] };
export type Ask = { id: string; question: string; options: string[] };
export type Part = { type: "text"; text: string } | { type: "proposal"; value: Proposal } | { type: "ask"; value: Ask };

const BLOCK = /```studio-(proposal|ask)\s*\n([\s\S]*?)```/g;

/** Разбить текст ответа на куски текста и карточки. Битый блок остается текстом. */
export function splitBlocks(text: string): Part[] {
  const parts: Part[] = [];
  let last = 0;
  for (const m of text.matchAll(BLOCK)) {
    const at = m.index ?? 0;
    if (at > last) parts.push({ type: "text", text: text.slice(last, at) });
    try {
      const value = JSON.parse(m[2] ?? "");
      if (m[1] === "proposal" && typeof value.id === "string" && typeof value.title === "string") parts.push({ type: "proposal", value });
      else if (m[1] === "ask" && typeof value.id === "string" && Array.isArray(value.options)) parts.push({ type: "ask", value });
      else parts.push({ type: "text", text: m[0] });
    } catch {
      parts.push({ type: "text", text: m[0] });
    }
    last = at + m[0].length;
  }
  if (last < text.length) parts.push({ type: "text", text: text.slice(last) });
  return parts.filter((p) => p.type !== "text" || p.text.trim() !== "");
}

/** Решение по карточке из служебной строки: `[proposal p1: accepted]`, `[answer q1] вариант`. */
export function parseDecision(text: string): { id: string; verdict: string } | null {
  const p = /^\[proposal ([\w-]+): (accepted|rejected|edited|comment)\]/.exec(text);
  if (p) return { id: p[1] ?? "", verdict: p[2] ?? "" };
  const a = /^\[answer ([\w-]+)\]\s*(.*)$/s.exec(text);
  if (a) return { id: a[1] ?? "", verdict: a[2] ?? "" };
  return null;
}
