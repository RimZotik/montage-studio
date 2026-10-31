import type { LucideIcon } from "lucide-react";

// Реестр команд (docs/ui.md): меню, горячие клавиши, палитра и позже Claude вызывают одно и то же.
// Команда описана один раз; меню ссылаются на нее по id.

export type Command = {
  id: string;
  /** Ключ перевода названия. */
  label: string;
  icon?: LucideIcon;
  /** Клавиши по умолчанию: «Ctrl+Shift+P». */
  keys?: string;
  /** Недоступна — серая в меню, не срабатывает по клавишам. */
  enabled?: () => boolean;
  run: () => void;
};

export type MenuItem = string | "-";

/** Сочетание клавиш события в виде «Ctrl+Shift+P». */
export function comboOf(event: Pick<KeyboardEvent, "ctrlKey" | "shiftKey" | "altKey" | "key" | "code">): string {
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.shiftKey) parts.push("Shift");
  if (event.altKey) parts.push("Alt");
  // По `code` — чтобы сочетания не зависели от раскладки (Ctrl+N и на русской).
  const code = event.code;
  let key = event.key;
  if (code.startsWith("Key")) key = code.slice(3);
  else if (code.startsWith("Digit")) key = code.slice(5);
  else if (code === "Backquote") key = "`";
  else if (code === "Comma") key = ",";
  else if (key.length === 1) key = key.toUpperCase();
  parts.push(key);
  return parts.join("+");
}

/** Поле ввода в фокусе — клавиши без Ctrl/Alt не перехватываются. */
export function typingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
}

export function findByKeys(commands: Command[], combo: string): Command | undefined {
  return commands.find((c) => c.keys === combo && (c.enabled?.() ?? true));
}
