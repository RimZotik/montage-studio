import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type RuleFile, commands as ipc } from "@/shared/ipc/bindings";

const SAVE_AFTER_MS = 600;

/** Правила проекта для Claude: бриф, правила, стиль, сценарий. Сохраняются сами через паузу в
 *  наборе; правки Claude подтягиваются после его ответа (`reloadKey`). */
export function RulesColumn({ onError, reloadKey }: { onError: (e: AppError) => void; reloadKey: number }) {
  const { t } = useTranslation();
  const [files, setFiles] = useState<RuleFile[]>([]);
  const [active, setActive] = useState("brief");
  const [state, setState] = useState<"saved" | "dirty" | "saving">("saved");
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    // Не затирать то, что пользователь печатает прямо сейчас.
    if (state !== "saved") return;
    void ipc.rulesRead().then((r) => (r.status === "ok" ? setFiles(r.data) : onError(r.error)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadKey]);

  const save = useCallback(
    async (name: string, text: string) => {
      setState("saving");
      const r = await ipc.rulesWrite(name, text);
      if (r.status === "error") onError(r.error);
      setState("saved");
    },
    [onError],
  );

  const current = files.find((f) => f.name === active);

  return (
    <section className="col rules">
      <div className="rtabs">
        <span className="colhead">{t("prepare.rules")}</span>
        {files.map((f) => (
          <button key={f.name} className={active === f.name ? "on" : ""} onClick={() => setActive(f.name)}>
            {t(`rules.${f.name}`)}
            {f.text.trim() === "" && <span className="dot" />}
          </button>
        ))}
        <span className="rstate">{t(`rules.state.${state}`)}</span>
      </div>
      <textarea
        className="ruletext"
        spellCheck={false}
        placeholder={t(`rules.hint.${active}`)}
        value={current?.text ?? ""}
        onChange={(e) => {
          const text = e.target.value;
          setFiles((all) => all.map((f) => (f.name === active ? { ...f, text } : f)));
          setState("dirty");
          if (timer.current) clearTimeout(timer.current);
          timer.current = setTimeout(() => void save(active, text), SAVE_AFTER_MS);
        }}
      />
    </section>
  );
}
