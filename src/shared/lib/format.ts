import type { AppError } from "@/shared/ipc/bindings";
import i18n from "@/shared/i18n";

/** Текст ошибки команды: перевод по коду, иначе общий текст с сообщением. */
export function errorText(error: AppError): string {
  const key = `error.${error.code}`;
  return i18n.exists(key) ? i18n.t(key) : i18n.t("error.unknown", { message: error.message });
}

/** Unix-секунды → «сегодня, 12:04» / «28 окт.» / «28 окт. 2025». */
export function shortDate(unix: number, lang: string, now = Date.now()): string {
  const d = new Date(unix * 1000);
  const today = new Date(now);
  const time = d.toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === today.toDateString()) return `${lang === "ru" ? "сегодня" : "today"}, ${time}`;
  const opts: Intl.DateTimeFormatOptions = d.getFullYear() === today.getFullYear() ? { day: "numeric", month: "short" } : { day: "numeric", month: "short", year: "numeric" };
  return d.toLocaleDateString(lang, opts);
}

/** Время сброса лимита: сегодня — «в 18:40», иначе дата и время. */
export function resetTime(unix: number, lang: string, now = Date.now()): string {
  const d = new Date(unix * 1000);
  const time = d.toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === new Date(now).toDateString()) return lang === "ru" ? `в ${time}` : `at ${time}`;
  return `${d.toLocaleDateString(lang, { day: "numeric", month: "short" })} ${time}`;
}

/** Формат кадра по размеру последовательности. */
export function aspect(width: number, height: number): string {
  if (width === height) return "1:1";
  return width > height ? "16:9" : "9:16";
}
