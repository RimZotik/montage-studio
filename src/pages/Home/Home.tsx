import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { CircleAlert, CircleCheck, CircleX, FolderOpen, FolderX, LayoutTemplate, Plus, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ProjectSummary, SystemReport, ToolStatus } from "@/shared/ipc/bindings";
import { aspect, shortDate } from "@/shared/lib/format";

const INSTALL: Record<ToolStatus["id"], string> = {
  claude: "https://docs.claude.com/en/docs/claude-code/setup",
  node: "https://nodejs.org/en/download",
  ffmpeg: "https://ffmpeg.org/download.html",
  ffprobe: "https://ffmpeg.org/download.html",
};

/** Цвет миниатюры проекта — постоянный по имени. */
function tint(name: string): string {
  const hues = ["#F28A4B", "#9A8BFF", "#5DBB8E", "#6F95FF", "#E0B23A", "#EE7C7F", "#38B2B2"];
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const c = hues[h % hues.length] ?? "#F28A4B";
  return `linear-gradient(170deg, ${c}33, ${c})`;
}

type Filter = "all" | "9:16" | "16:9" | "1:1";

export type HomeProps = {
  report: SystemReport | null;
  checking: boolean;
  projects: ProjectSummary[];
  query: string;
  onNew: () => void;
  onOpenFolder: () => void;
  onOpen: (p: ProjectSummary) => void;
  onForget: (p: ProjectSummary) => void;
  onRecheck: () => void;
};

function ReadyRow({ tool }: { tool: ToolStatus }) {
  const { t } = useTranslation();
  const state = !tool.found ? "miss" : tool.error ? "err" : "ok";
  const Icon = state === "ok" ? CircleCheck : state === "err" ? CircleX : CircleAlert;
  const value = state === "ok" ? tool.version : state === "err" ? t("tool.broken") : null;
  return (
    <div className={`ready ${state}`} title={tool.path ?? tool.error ?? undefined}>
      <Icon size={14} strokeWidth={1.75} />
      <span>{t(`tool.${tool.id}`)}</span>
      {value ? (
        <span className="v">{value}</span>
      ) : (
        <button className="link" style={{ marginLeft: "auto" }} onClick={() => void openUrl(INSTALL[tool.id])}>
          {t("home.install")}
        </button>
      )}
    </div>
  );
}

export function Home(props: HomeProps) {
  const { t, i18n } = useTranslation();
  const [filter, setFilter] = useState<Filter>("all");
  const shown = useMemo(() => {
    const q = props.query.trim().toLowerCase();
    return props.projects.filter((p) => (filter === "all" || aspect(p.width, p.height) === filter) && (!q || p.name.toLowerCase().includes(q) || p.path.toLowerCase().includes(q)));
  }, [props.projects, props.query, filter]);
  const monthAgo = Date.now() / 1000 - 30 * 86400;
  const hw = props.report?.hardware;

  return (
    <div className="home">
      <div className="start">
        <h1>{t("home.start")}</h1>
        <div className="ver">{t("home.version", { version: __APP_VERSION__ })}</div>
        <button className="act" onClick={props.onNew}>
          <Plus size={16} strokeWidth={1.75} />
          {t("home.newProject")}
          <span className="k">Ctrl+N</span>
        </button>
        <button className="act" onClick={props.onOpenFolder}>
          <FolderOpen size={16} strokeWidth={1.75} />
          {t("home.openFolder")}
          <span className="k">Ctrl+O</span>
        </button>
        <button className="act" disabled style={{ opacity: 0.5, cursor: "default" }}>
          <LayoutTemplate size={16} strokeWidth={1.75} />
          {t("home.fromTemplate")}
          <span className="k">{t("home.soon")}</span>
        </button>

        <div className="sh">
          {t("home.ready")}
          <button className="iconbtn" title={t("home.recheck")} disabled={props.checking} onClick={props.onRecheck}>
            <RefreshCw size={13} strokeWidth={1.75} className={props.checking ? "spin" : ""} />
          </button>
        </div>
        {props.report ? props.report.tools.map((tool) => <ReadyRow key={tool.id} tool={tool} />) : <div className="ready muted">{t("home.checking")}</div>}
        {hw && (
          <div className="hw">
            {t("home.hw", { cpu: hw.cpu, threads: hw.threads, memory: hw.memoryGb })}
            {hw.gpus.map((g) => (
              <div key={g}>{t("home.gpu", { gpu: g })}</div>
            ))}
          </div>
        )}
      </div>

      <div className="recent">
        <div className="rh">
          <h2>{t("home.recent")}</h2>
          <div className="flt">
            {(["all", "9:16", "16:9", "1:1"] as const).map((f) => (
              <button key={f} className={filter === f ? "on" : ""} onClick={() => setFilter(f)}>
                {f === "all" ? t("home.all") : f}
              </button>
            ))}
          </div>
        </div>

        {props.projects.length === 0 ? (
          <div className="empty">
            <b style={{ color: "var(--text)", fontWeight: 500 }}>{t("home.emptyTitle")}</b>
            {t("home.emptyText")}
            <button className="btn pri" onClick={props.onNew}>
              <Plus size={14} strokeWidth={1.75} />
              {t("home.newProject")}
            </button>
          </div>
        ) : (
          <div className="grid">
            {shown.map((p) => {
              const fmt = aspect(p.width, p.height);
              const frame = fmt === "9:16" ? { height: "78%", aspectRatio: "9 / 16" } : fmt === "16:9" ? { width: "72%", aspectRatio: "16 / 9" } : { height: "70%", aspectRatio: "1" };
              return (
                <div key={p.path} className="pr">
                  <button
                    className="th"
                    style={{ width: "100%" }}
                    title={p.path}
                    onClick={() => (p.exists ? props.onOpen(p) : undefined)}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      if (p.exists) void revealItemInDir(p.path);
                    }}
                  >
                    <span className="fmt">
                      {fmt} · {p.fps}
                    </span>
                    <i style={{ ...frame, background: tint(p.name) }} />
                    {!p.exists && (
                      <span className="gone">
                        <FolderX size={14} strokeWidth={1.75} />
                        {t("home.missing")}
                      </span>
                    )}
                  </button>
                  <div className="n">{p.name}</div>
                  <div className="m">
                    {shortDate(p.openedAt, i18n.language)}
                    {!p.exists && (
                      <>
                        {" · "}
                        <button className="link" onClick={() => props.onForget(p)}>
                          {t("home.forget")}
                        </button>
                      </>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        <div className="stats">
          <div className="stat">
            <div className="v">{props.projects.length}</div>
            <div className="l">{t("home.statProjects")}</div>
          </div>
          <div className="stat">
            <div className="v">{props.projects.filter((p) => p.openedAt > monthAgo).length}</div>
            <div className="l">{t("home.statThisMonth")}</div>
          </div>
          <div className="stat">
            <div className="v">—</div>
            <div className="l">{t("home.statClaude")}</div>
          </div>
          <div className="stat">
            <div className="v">0</div>
            <div className="l">{t("home.statTools")}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
