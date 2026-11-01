import { RefreshCw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { LimitWindow, Limits } from "@/shared/ipc/bindings";
import { resetTime, shortDate } from "@/shared/lib/format";

/** Доля 0–1 (число может прийти пустым). */
const used = (w: LimitWindow | null | undefined) => w?.utilization ?? 0;

function level(w: LimitWindow | null | undefined): "" | "warn" | "bad" {
  if (!w) return "";
  if (w.status === "rejected" || used(w) >= 1) return "bad";
  if (w.status === "allowed_warning" || used(w) >= 0.8) return "warn";
  return "";
}

const pct = (w: LimitWindow | null | undefined) => (w ? `${Math.round(used(w) * 100)}%` : "—");

/** Индикатор лимитов Claude во второй строке и панель с подробностями по клику. */
export function LimitsIndicator({ limits, refreshing, onRefresh }: { limits: Limits | null; refreshing: boolean; onRefresh: () => void }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const lang = i18n.language;

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const row = (label: string, w: LimitWindow | null | undefined) => (
    <div className="pl">
      <div className="r">
        <span>{label}</span>
        <span>
          {pct(w)}
          {w && ` · ${t("limits.resetsAt", { when: resetTime(w.resetsAt, lang) })}`}
        </span>
      </div>
      <div className="bar">
        <i className={level(w)} style={{ width: `${Math.min(100, used(w) * 100)}%` }} />
      </div>
    </div>
  );

  return (
    <div className="lim" ref={ref} onClick={() => setOpen((o) => !o)} role="button">
      {(["session", "week"] as const).map((k) => {
        const w = k === "session" ? limits?.fiveHour : limits?.sevenDay;
        return (
          <div className="b" key={k}>
            <div className="t">
              <span>{t(`limits.${k}`)}</span>
              <b>{pct(w)}</b>
            </div>
            <div className="bar">
              <i className={level(w)} style={{ width: `${Math.min(100, used(w) * 100)}%` }} />
            </div>
          </div>
        );
      })}
      {open && (
        <div className="pop" onClick={(e) => e.stopPropagation()}>
          <h4>{t("limits.title")}</h4>
          {limits ? (
            <>
              {row(t("limits.sessionLong"), limits.fiveHour)}
              {row(t("limits.weekLong"), limits.sevenDay)}
              <div className="note">{t("limits.updated", { when: shortDate(limits.at, lang) })}</div>
            </>
          ) : (
            <div className="note">{t("limits.noneHint")}</div>
          )}
          <button className="btn" disabled={refreshing} onClick={onRefresh}>
            <RefreshCw size={14} strokeWidth={1.75} className={refreshing ? "spin" : ""} />
            {refreshing ? t("limits.refreshing") : t("limits.refresh")}
          </button>
        </div>
      )}
    </div>
  );
}
