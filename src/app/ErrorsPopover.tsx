import { CircleX, Sparkles } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type ErrorEntry, commands as ipc } from "@/shared/ipc/bindings";
import { errorText } from "@/shared/lib/format";

/** Окошко ошибок из строки состояния: значок важности, время, текст; «Пропустить» — очистить,
 *  «Спросить Claude» — он читает журнал и подсказывает причину (только по этой кнопке). */
export function ErrorsPopover({ count, canAsk, onAsk, onCleared }: { count: number; canAsk: boolean; onAsk: () => void; onCleared: () => void }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<ErrorEntry[]>([]);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    void ipc.errorsRecent().then(setList);
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  if (count === 0) return null;
  return (
    <div className="errwrap" ref={ref}>
      <button className="ic errs" onClick={() => setOpen((o) => !o)}>
        <CircleX size={12} strokeWidth={1.75} />
        {t("status.errors", { count })}
      </button>
      {open && (
        <div className="errpop">
          <div className="errlist">
            {list.map((e, i) => (
              <div key={`${e.at}-${i}`} className="errrow">
                <CircleX size={14} strokeWidth={1.75} />
                <div>
                  <div className="et">{errorText({ code: e.code, message: e.message })}</div>
                  <div className="em mono">
                    {new Date(e.at * 1000).toLocaleTimeString(i18n.language)} · {e.code}
                  </div>
                </div>
              </div>
            ))}
          </div>
          <div className="errbtns">
            <button
              className="btn quiet"
              onClick={() => {
                void ipc.errorsClear().then(onCleared);
                setOpen(false);
              }}
            >
              {t("status.skip")}
            </button>
            <button
              className="btn pri"
              disabled={!canAsk}
              title={canAsk ? undefined : t("status.needProject")}
              onClick={() => {
                setOpen(false);
                onAsk();
              }}
            >
              <Sparkles size={13} strokeWidth={1.75} />
              {t("status.askClaude")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
