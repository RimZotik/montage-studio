import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, type CacheEntry, type Settings, commands as ipc } from "@/shared/ipc/bindings";
import { resetSettings, useSetting } from "@/shared/settings/settings";

const size = (bytes: number) => (bytes > 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} ГБ` : `${Math.round(bytes / 1024 ** 2)} МБ`);

/** Настройки программы и сколько занимает кэш (`docs/settings.md`, `docs/storage.md`). */
export function SettingsDialog({ settings, onSave, onClose, onError }: { settings: Settings; onSave: (next: Settings) => void; onClose: () => void; onError: (e: AppError) => void }) {
  const { t, i18n } = useTranslation();
  const [safe, setSafe] = useSetting("monitor.safeZone", true);
  const [snap, setSnap] = useSetting("timeline.snap", true);
  const [cache, setCache] = useState<CacheEntry[]>([]);
  const load = useCallback(() => void ipc.cacheUsage().then((r) => (r.status === "ok" ? setCache(r.data) : onError(r.error))), [onError]);
  useEffect(load, [load]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);

  const chooseDir = async () => {
    const dir = await openDialog({ directory: true, defaultPath: settings.projectsDir ?? undefined });
    if (typeof dir === "string") onSave({ ...settings, projectsDir: dir });
  };
  const chooseTemp = async () => {
    const dir = await openDialog({ directory: true, defaultPath: settings.tempDir ?? undefined });
    if (typeof dir === "string") onSave({ ...settings, tempDir: dir });
  };

  return (
    <div className="overlay" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dlg setdlg">
        <div className="hd">
          <h3>{t("settings.title")}</h3>
          <button className="iconbtn" title={t("library.close")} onClick={onClose}>
            <X size={15} strokeWidth={1.75} />
          </button>
        </div>
        <div className="bd setbody">
          <label className="setrow">
            <span>{t("settings.language")}</span>
            <select
              className="field"
              value={i18n.language}
              onChange={(e) => {
                void i18n.changeLanguage(e.target.value);
                onSave({ ...settings, language: e.target.value });
              }}
            >
              <option value="ru">Русский</option>
              <option value="en">English</option>
            </select>
          </label>
          <label className="setrow">
            <span>{t("settings.model")}</span>
            <select className="field" value={settings.claudeModel} onChange={(e) => onSave({ ...settings, claudeModel: e.target.value })}>
              {["auto", "sonnet", "opus", "haiku"].map((m) => (
                <option key={m} value={m}>
                  {t(`chat.models.${m}`)}
                </option>
              ))}
            </select>
          </label>
          <div className="setrow">
            <span>{t("settings.projectsDir")}</span>
            <div className="setpath">
              <span title={settings.projectsDir ?? ""}>{settings.projectsDir ?? t("settings.notSet")}</span>
              <button className="iconbtn" title={t("export.folderChoose")} onClick={() => void chooseDir()}>
                <FolderOpen size={14} strokeWidth={1.75} />
              </button>
            </div>
          </div>
          <div className="setrow">
            <span>{t("settings.tempDir")}</span>
            <div className="setpath">
              <span title={settings.tempDir ?? t("settings.tempDirHint")}>{settings.tempDir ?? t("settings.tempDirDefault")}</span>
              <button className="iconbtn" title={t("export.folderChoose")} onClick={() => void chooseTemp()}>
                <FolderOpen size={14} strokeWidth={1.75} />
              </button>
              {settings.tempDir && (
                <button className="iconbtn" title={t("settings.tempDirReset")} onClick={() => onSave({ ...settings, tempDir: null })}>
                  <X size={14} strokeWidth={1.75} />
                </button>
              )}
            </div>
          </div>
          <label className="setrow check">
            <input type="checkbox" checked={safe} onChange={(e) => setSafe(e.target.checked)} />
            <span>{t("settings.safeZone")}</span>
          </label>
          <label className="setrow check">
            <input type="checkbox" checked={snap} onChange={(e) => setSnap(e.target.checked)} />
            <span>{t("settings.snap")}</span>
          </label>

          <div className="ctlsec">{t("settings.cache")}</div>
          <p className="faint fxdesc">{t("settings.cacheHint")}</p>
          {cache.map((c) => (
            <div key={c.id} className="setcache">
              <span>{t(`settings.caches.${c.id}`)}</span>
              <span className="faint mono">
                {size(c.bytes)} · {c.files}
              </span>
              <button className="iconbtn" title={t("settings.clear")} disabled={!c.files} onClick={() => void ipc.cacheClear(c.id).then((r) => (r.status === "error" ? onError(r.error) : load()))}>
                <Trash2 size={14} strokeWidth={1.75} />
              </button>
            </div>
          ))}
        </div>
        <div className="ft">
          <button className="btn quiet" style={{ marginRight: "auto" }} onClick={resetSettings}>
            {t("settings.reset")}
          </button>
          <button className="btn pri" onClick={onClose}>
            {t("settings.done")}
          </button>
        </div>
      </div>
    </div>
  );
}
