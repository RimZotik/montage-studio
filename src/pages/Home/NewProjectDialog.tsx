import { open } from "@tauri-apps/plugin-dialog";
import { X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type Preset, type ProjectSummary } from "@/shared/ipc/bindings";
import { errorText } from "@/shared/lib/format";

const PRESETS: { id: Preset; size: string; shape: { width: number; height: number } }[] = [
  { id: "vertical", size: "1080×1920", shape: { width: 14, height: 24 } },
  { id: "horizontal", size: "1920×1080", shape: { width: 26, height: 15 } },
  { id: "square", size: "1080×1080", shape: { width: 20, height: 20 } },
];
const FPS = [24, 25, 30, 50, 60];

export function NewProjectDialog({ defaultParent, onClose, onCreated }: { defaultParent: string | null; onClose: () => void; onCreated: (p: ProjectSummary, parent: string) => void }) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [parent, setParent] = useState(defaultParent ?? "");
  const [preset, setPreset] = useState<Preset>("vertical");
  const [fps, setFps] = useState(60);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const choose = async () => {
    const dir = await open({ directory: true, title: t("newProject.chooseFolder"), defaultPath: parent || undefined });
    if (typeof dir === "string") setParent(dir);
  };

  const create = async () => {
    setBusy(true);
    const r = await commands.projectCreate({ name, parent, preset, fps });
    setBusy(false);
    if (r.status === "ok") onCreated(r.data, parent);
    else setError(errorText(r.error));
  };

  const ready = name.trim() !== "" && parent !== "" && !busy;

  return (
    <div className="overlay" onMouseDown={onClose}>
      <div className="dlg" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <div className="hd">
          <h3>{t("newProject.title")}</h3>
          <button className="iconbtn" onClick={onClose} aria-label={t("newProject.cancel")}>
            <X size={16} strokeWidth={1.75} />
          </button>
        </div>
        <div className="bd">
          <div className="lbl">{t("newProject.name")}</div>
          <input className="field" autoFocus placeholder={t("newProject.namePlaceholder")} value={name} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === "Enter" && ready && void create()} />
          <div className="lbl">{t("newProject.folder")}</div>
          <div className="pathrow">
            <input className="field" readOnly value={parent} onClick={() => void choose()} />
            <button className="btn" onClick={() => void choose()}>
              {t("newProject.choose")}
            </button>
          </div>
          <div className="lbl">{t("newProject.format")}</div>
          <div className="presets">
            {PRESETS.map((p) => (
              <button key={p.id} className={`preset ${preset === p.id ? "on" : ""}`} onClick={() => setPreset(p.id)}>
                <span className="shape" style={p.shape} />
                <span>
                  <b>{t(`newProject.${p.id}`)}</b>
                  <small>{p.size}</small>
                </span>
              </button>
            ))}
          </div>
          <div className="lbl">{t("newProject.fps")}</div>
          <div className="seg">
            {FPS.map((f) => (
              <button key={f} className={fps === f ? "on" : ""} onClick={() => setFps(f)}>
                {f}
              </button>
            ))}
          </div>
        </div>
        <div className="ft">
          {error && <span className="err">{error}</span>}
          <button className="btn quiet" onClick={onClose}>
            {t("newProject.cancel")}
          </button>
          <button className="btn pri" disabled={!ready} onClick={() => void create()}>
            {t("newProject.create")}
          </button>
        </div>
      </div>
    </div>
  );
}
