import { appLogDir } from "@tauri-apps/api/path";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CircleCheck, CircleX, Command as CommandIcon, Download, FileText, FolderOpen, Gauge, Languages, LogOut, Plus, Redo2, RefreshCw, RotateCw, Search, TriangleAlert, Undo2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { NewProjectDialog } from "@/pages/Home/NewProjectDialog";
import { Home } from "@/pages/Home/Home";
import { pickLanguage } from "@/shared/i18n";
import { type AppError, type ComponentId, type ComponentProgress, type Limits, type ProjectSummary, type Settings, type SystemReport, type UpdateStatus, commands as ipc, events } from "@/shared/ipc/bindings";
import { errorText } from "@/shared/lib/format";
import { type Command, comboOf, findByKeys, typingTarget } from "./commands";
import { LimitsIndicator } from "./Limits";
import { Palette } from "./Palette";
import { type MenuSpec, TitleBar } from "./TitleBar";

type Tab = "home" | "projects" | "library";
type Toast = { id: number; text: string; error?: boolean };

export function App() {
  const { t, i18n } = useTranslation();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [tab, setTab] = useState<Tab>("home");
  const [report, setReport] = useState<SystemReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [limits, setLimits] = useState<Limits | null>(null);
  const [limitsBusy, setLimitsBusy] = useState(false);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<"new" | "palette" | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [installs, setInstalls] = useState<Partial<Record<ComponentId, ComponentProgress>>>({});
  const [update, setUpdate] = useState<UpdateStatus | null>(null);

  const toast = useCallback((text: string, error = false) => {
    const id = Date.now() + Math.random();
    setToasts((list) => [...list, { id, text, error }]);
    setTimeout(() => setToasts((list) => list.filter((x) => x.id !== id)), 5000);
  }, []);
  const fail = useCallback((e: AppError) => toast(errorText(e), true), [toast]);

  const check = useCallback(async () => {
    setChecking(true);
    const r = await ipc.systemCheck();
    setChecking(false);
    if (r.status === "ok") setReport(r.data);
    else fail(r.error);
  }, [fail]);

  const loadProjects = useCallback(async () => {
    const r = await ipc.projectsRecent();
    if (r.status === "ok") setProjects(r.data);
  }, []);

  const refreshLimits = useCallback(async () => {
    setLimitsBusy(true);
    const r = await ipc.claudeLimitsRefresh();
    setLimitsBusy(false);
    if (r.status === "ok") setLimits(r.data);
    else fail(r.error);
  }, [fail]);

  const saveSettings = useCallback(async (next: Settings) => {
    const r = await ipc.settingsUpdate(next);
    if (r.status === "ok") setSettings(r.data);
  }, []);

  useEffect(() => {
    void (async () => {
      const s = await ipc.settingsGet();
      setSettings(s);
      await i18n.changeLanguage(pickLanguage(s.language ?? ""));
      const l = await ipc.claudeLimitsLast();
      if (l.status === "ok") setLimits(l.data);
    })();
    void loadProjects();
    void check();
    void ipc.updateStatus().then(setUpdate);
  }, [check, loadProjects, i18n]);

  // Ход установки компонентов; конец — перепроверка системы.
  useEffect(() => {
    const off = events.componentProgress.listen(({ payload }) => {
      setInstalls((all) => ({ ...all, [payload.id]: payload }));
      if (payload.phase === "done") void check();
    });
    const offUpdate = events.updateChanged.listen(({ payload }) => setUpdate(payload));
    return () => {
      void off.then((f) => f());
      void offUpdate.then((f) => f());
    };
  }, [check]);

  const install = useCallback(
    async (id: ComponentId) => {
      setInstalls((all) => ({ ...all, [id]: { id, phase: "download", received: 0, total: 1, error: null } }));
      const r = await ipc.componentInstall(id);
      if (r.status === "error") fail(r.error);
    },
    [fail],
  );

  const openProject = useCallback(
    async (path: string) => {
      const r = await ipc.projectOpen(path);
      if (r.status === "ok") {
        toast(t("home.opened", { name: r.data.name }));
        void loadProjects();
      } else fail(r.error);
    },
    [fail, loadProjects, t, toast],
  );

  const openFolder = useCallback(async () => {
    const dir = await openDialog({ directory: true });
    if (typeof dir === "string") await openProject(dir);
  }, [openProject]);

  const toggleLanguage = useCallback(async () => {
    const next = i18n.language === "ru" ? "en" : "ru";
    await i18n.changeLanguage(next);
    if (settings) await saveSettings({ ...settings, language: next });
  }, [i18n, saveSettings, settings]);

  const commandList: Command[] = useMemo(
    () => [
      { id: "newProject", label: "cmd.newProject", icon: Plus, keys: "Ctrl+N", run: () => setDialog("new") },
      { id: "openProject", label: "cmd.openProject", icon: FolderOpen, keys: "Ctrl+O", run: () => void openFolder() },
      { id: "undo", label: "cmd.undo", icon: Undo2, keys: "Ctrl+Z", enabled: () => false, run: () => {} },
      { id: "redo", label: "cmd.redo", icon: Redo2, keys: "Ctrl+Shift+Z", enabled: () => false, run: () => {} },
      { id: "palette", label: "cmd.palette", icon: CommandIcon, keys: "Ctrl+Shift+P", run: () => setDialog("palette") },
      { id: "home", label: "cmd.home", keys: "Alt+1", run: () => setTab("home") },
      { id: "projects", label: "cmd.projects", keys: "Alt+2", run: () => setTab("projects") },
      { id: "library", label: "cmd.library", keys: "Alt+3", run: () => setTab("library") },
      { id: "language", label: "cmd.language", icon: Languages, run: () => void toggleLanguage() },
      { id: "reload", label: "cmd.reload", icon: RotateCw, keys: "Ctrl+R", run: () => window.location.reload() },
      { id: "refreshChecks", label: "cmd.refreshChecks", icon: RefreshCw, keys: "F5", run: () => void check() },
      { id: "refreshLimits", label: "cmd.refreshLimits", icon: Gauge, run: () => void refreshLimits() },
      { id: "openLogs", label: "cmd.openLogs", icon: FileText, run: () => void appLogDir().then((d) => revealItemInDir(d)) },
      { id: "checkUpdates", label: "cmd.checkUpdates", icon: Download, run: () => void ipc.updateCheckNow() },
      { id: "installUpdate", label: "cmd.installUpdate", icon: Download, enabled: () => update?.phase === "ready", run: () => void ipc.updateInstall().then((r) => r.status === "error" && fail(r.error)) },
      { id: "quit", label: "cmd.quit", icon: LogOut, keys: "Alt+F4", run: () => void getCurrentWindow().close() },
    ],
    [check, fail, openFolder, refreshLimits, toggleLanguage, update?.phase],
  );
  const byId = useMemo(() => new Map(commandList.map((c) => [c.id, c])), [commandList]);

  const menus: MenuSpec[] = [
    { id: "file", label: "menu.file", items: ["newProject", "openProject", "-", "quit"] },
    { id: "edit", label: "menu.edit", items: ["undo", "redo", "-", "palette"] },
    { id: "view", label: "menu.view", items: ["home", "projects", "library", "-", "language", "reload"] },
    { id: "window", label: "menu.window", items: ["refreshChecks", "refreshLimits"] },
    { id: "help", label: "menu.help", items: ["checkUpdates", "installUpdate", "-", "openLogs"] },
  ];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (dialog) return;
      if (typingTarget(e.target) && !e.ctrlKey && !e.altKey) return;
      const c = findByKeys(commandList, comboOf(e));
      if (c) {
        e.preventDefault();
        c.run();
      }
    };
    // Браузерное контекстное меню — только в полях ввода.
    const onContext = (e: MouseEvent) => !typingTarget(e.target) && e.preventDefault();
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onContext);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onContext);
    };
  }, [commandList, dialog]);

  const missing = report?.tools.filter((x) => !x.found || x.error) ?? [];
  const claude = report?.tools.find((x) => x.id === "claude");

  return (
    <div className="app">
      <TitleBar menus={menus} commands={byId} />

      <div className="pagebar">
        <div className="tabs">
          {(["home", "projects", "library"] as const).map((k) => (
            <button key={k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>
              {t(`tabs.${k}`)}
            </button>
          ))}
        </div>
        <div className="pb-right">
          <label className="search">
            <Search size={14} strokeWidth={1.75} />
            <input placeholder={t("search.projects")} value={query} onChange={(e) => setQuery(e.target.value)} />
          </label>
          {update?.phase === "ready" && update.version && (
            <button className="btn" title={update.notes ?? undefined} onClick={() => byId.get("installUpdate")?.run()}>
              <Download size={14} strokeWidth={1.75} />
              {t("update.install", { version: update.version })}
            </button>
          )}
          <LimitsIndicator limits={limits} refreshing={limitsBusy} onRefresh={() => void refreshLimits()} />
          <button className="btn pri" onClick={() => setDialog("new")}>
            <Plus size={14} strokeWidth={2} />
            {t("home.newProject")}
          </button>
        </div>
      </div>

      {tab === "home" ? (
        <Home
          report={report}
          checking={checking}
          installs={installs}
          onInstall={(id) => void install(id)}
          onCancelInstall={(id) => void ipc.componentCancel(id)}
          projects={projects}
          query={query}
          onNew={() => setDialog("new")}
          onOpenFolder={() => void openFolder()}
          onOpen={(p) => void openProject(p.path)}
          onForget={(p) => void ipc.projectForget(p.path).then(loadProjects)}
          onRecheck={() => void check()}
        />
      ) : (
        <div className="soon">{t("soon.page")}</div>
      )}

      <div className="status">
        {!report ? (
          <span className="ic">{t("status.checking")}</span>
        ) : missing.length === 0 ? (
          <span className="ic ok">
            <CircleCheck size={12} strokeWidth={1.75} />
            {t("status.allReady")}
          </span>
        ) : (
          <span className="ic warn">
            <TriangleAlert size={12} strokeWidth={1.75} />
            {t("status.missingCount", { list: missing.map((x) => t(`tool.${x.id}`)).join(", ") })}
          </span>
        )}
        {update?.phase === "downloading" && (
          <span className="ic">
            <Download size={12} strokeWidth={1.75} />
            {t("update.downloading", { version: update.version, pct: Math.round((update.progress ?? 0) * 100) })}
          </span>
        )}
        {claude && <span className="ic">{claude.version ? t("status.claude", { version: claude.version }) : t("status.noClaude")}</span>}
        <button className="ic end" onClick={() => void toggleLanguage()}>
          <Languages size={12} strokeWidth={1.75} />
          {i18n.language.toUpperCase()}
        </button>
      </div>

      {dialog === "new" && (
        <NewProjectDialog
          defaultParent={settings?.projectsDir ?? null}
          onClose={() => setDialog(null)}
          onCreated={(p, parent) => {
            setDialog(null);
            toast(t("home.created", { name: p.name }));
            void loadProjects();
            if (settings && settings.projectsDir !== parent) void saveSettings({ ...settings, projectsDir: parent });
          }}
        />
      )}
      {dialog === "palette" && <Palette commands={commandList} onClose={() => setDialog(null)} />}

      <div className="toasts">
        {toasts.map((x) => (
          <div key={x.id} className={`toast ${x.error ? "error" : ""}`}>
            {x.error ? <CircleX size={16} strokeWidth={1.75} /> : <CircleCheck size={16} strokeWidth={1.75} />}
            {x.text}
          </div>
        ))}
      </div>
    </div>
  );
}
