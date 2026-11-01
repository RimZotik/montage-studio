import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { FolderOpen, FolderX, X } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { ProjectSummary } from "@/shared/ipc/bindings";
import { aspect, shortDate } from "@/shared/lib/format";

/** «Проекты»: все известные проекты таблицей — открыть, показать в папке, убрать из списка. */
export function Projects({ projects, query, onOpen, onForget }: { projects: ProjectSummary[]; query: string; onOpen: (p: ProjectSummary) => void; onForget: (p: ProjectSummary) => void }) {
  const { t, i18n } = useTranslation();
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return projects.filter((p) => !q || p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q));
  }, [projects, query]);

  return (
    <div className="plist">
      <table>
        <thead>
          <tr>
            <th>{t("projects.name")}</th>
            <th>{t("projects.format")}</th>
            <th>{t("projects.opened")}</th>
            <th>{t("projects.created")}</th>
            <th>{t("projects.folder")}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {shown.map((p) => (
            <tr key={p.path} className={p.exists ? "" : "gone"} onDoubleClick={() => p.exists && onOpen(p)}>
              <td className="nm">
                {p.exists ? (
                  <button className="ic" onClick={() => onOpen(p)}>
                    {p.name}
                  </button>
                ) : (
                  <span className="ic">
                    <FolderX size={14} strokeWidth={1.75} />
                    {p.name}
                  </span>
                )}
              </td>
              <td className="mono">
                {aspect(p.width, p.height)} · {p.width}×{p.height} · {p.fps}
              </td>
              <td>{shortDate(p.openedAt, i18n.language)}</td>
              <td>{shortDate(p.createdAt, i18n.language)}</td>
              <td className="path" title={p.path}>
                {p.path}
              </td>
              <td className="acts">
                {p.exists && (
                  <button className="iconbtn" title={t("home.reveal")} onClick={() => void revealItemInDir(p.path)}>
                    <FolderOpen size={14} strokeWidth={1.75} />
                  </button>
                )}
                <button className="iconbtn" title={t("home.forget")} onClick={() => onForget(p)}>
                  <X size={14} strokeWidth={1.75} />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {shown.length === 0 && <div className="empty">{t(projects.length ? "projects.nothing" : "home.emptyText")}</div>}
    </div>
  );
}
