import { appLogDir } from "@tauri-apps/api/path";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { CircleCheck, CircleX, Command as CommandIcon, Download, FileText, FolderClosed, FolderOpen, Gauge, Languages, LogOut, MessageSquare, Plus, Redo2, RefreshCw, RotateCw, Search, TriangleAlert, Undo2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { NewProjectDialog } from "@/pages/Home/NewProjectDialog";
import { Home } from "@/pages/Home/Home";
import { Prepare } from "@/pages/Project/Prepare";
import { Projects } from "@/pages/Projects/Projects";
import { ChatPanel } from "@/panels/Claude/ChatPanel";
import { EditPage } from "@/pages/Edit/EditPage";
import { pickLanguage } from "@/shared/i18n";
import { type AppError, type ComponentId, type ComponentProgress, type Limits, type ProjectSummary, type ProjectView, type Settings, type SystemReport, type UpdateStatus, commands as ipc, events } from "@/shared/ipc/bindings";
import { errorText } from "@/shared/lib/format";
import { Splitter, useWidths } from "@/shared/ui/Splitter";
import { type Command, comboOf, findByKeys, typingTarget } from "./commands";
import { LimitsIndicator } from "./Limits";
import { Palette } from "./Palette";
import { type MenuSpec, TitleBar } from "./TitleBar";

type HomeTab = "home" | "projects" | "library";
type ProjectTab = "prepare" | "edit" | "export";
type Tab = HomeTab | ProjectTab;
const HOME_TABS: HomeTab[] = ["home", "projects", "library"];
const PROJECT_TABS: ProjectTab[] = ["prepare", "edit", "export"];
/** Лимиты старше этого обновляются при запуске сами (один короткий запрос). */
const LIMITS_STALE_S = 15 * 60;
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
  const [project, setProject] = useState<ProjectView | null>(null);
  const [limitsLoaded, setLimitsLoaded] = useState(false);
  const [autoLimits, setAutoLimits] = useState(false);
  const [chatOpen, setChatOpen] = useState(true);
  const [rulesKey, setRulesKey] = useState(0);
  const [mediaKey, setMediaKey] = useState(0);
  const [dragOver, setDragOver] = useState(false);
  const [claudeBusy, setClaudeBusy] = useState(false);
  const [errors, setErrors] = useState(0);
  const [ws, setWs] = useWidths("workspace", { chat: 400 });

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

  /** `silent` — без сообщения об ошибке (автообновление при запуске). */
  const refreshLimits = useCallback(
    async (silent = false) => {
      setLimitsBusy(true);
      const r = await ipc.claudeLimitsRefresh();
      setLimitsBusy(false);
      if (r.status === "ok") setLimits(r.data);
      else if (!silent) fail(r.error);
    },
    [fail],
  );

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
      setLimitsLoaded(true);
    })();
    void loadProjects();
    void check();
    void ipc.updateStatus().then(setUpdate);
  }, [check, loadProjects, i18n]);

  // При запуске: Claude Code найден, а лимиты устарели или их нет — обновить один раз.
  const claudeFound = !!report?.tools.find((x) => x.id === "claude" && x.found && !x.error);
  useEffect(() => {
    if (autoLimits || !limitsLoaded || !claudeFound) return;
    setAutoLimits(true);
    if (!limits || Date.now() / 1000 - limits.at > LIMITS_STALE_S) void refreshLimits(true);
  }, [autoLimits, limitsLoaded, claudeFound, limits, refreshLimits]);

  // Открытый проект меняет только ядро — сюда приходит его новое состояние.
  useEffect(() => {
    const off = events.projectChanged.listen(({ payload }) => setProject(payload));
    // Лимиты приходят с каждым ответом Claude в чате.
    const offLimits = events.limitsChanged.listen(({ payload }) => setLimits(payload));
    // После ответа Claude правила проекта могли измениться — перечитать; блокировка снимается.
    const offTurn = events.chatEvent.listen(({ payload }) => {
      if (payload.kind !== "turn") return;
      setRulesKey((k) => k + 1);
      setClaudeBusy(false);
    });
    // Команды Claude интерфейсу: вкладка, блокировка области на время его правок.
    const offUi = events.uiCommand.listen(({ payload: c }) => {
      if (c.kind === "tab") setTab(c.tab as Tab);
      if (c.kind === "busy") setClaudeBusy(true);
    });
    // Ошибки журнала с запуска — значок в строке состояния.
    const timer = setInterval(() => void ipc.errorsCount().then(setErrors), 5000);
    return () => {
      void off.then((f) => f());
      void offLimits.then((f) => f());
      void offTurn.then((f) => f());
      void offUi.then((f) => f());
      clearInterval(timer);
    };
  }, []);

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
      const r = await ipc.projectLoad(path);
      if (r.status === "ok") {
        setProject(r.data);
        setTab("prepare");
        void loadProjects();
      } else fail(r.error);
    },
    [fail, loadProjects],
  );

  const closeProject = useCallback(async () => {
    await ipc.projectClose();
    setProject(null);
    setTab("home");
  }, []);

  /** Импорт файлов в проект: итог одним сообщением, занятые и неподходящие файлы — по имени. */
  const importFiles = useCallback(
    async (files: string[]) => {
      if (!files.length) return;
      const r = await ipc.mediaImport(files);
      if (r.status === "error") return fail(r.error);
      const { imported, skipped } = r.data;
      if (imported) toast(t("prepare.imported", { count: imported }));
      for (const s of skipped) toast(t(`prepare.skipped.${s.reason}`, { name: s.name, defaultValue: t("prepare.skipped.io", { name: s.name }) }), true);
      setMediaKey((k) => k + 1);
    },
    [fail, t, toast],
  );

  // Файлы, перетащенные в окно открытого проекта: подсветка всего окна и импорт. Подписка
  // снимается и тогда, когда эффект закрылся раньше, чем она оформилась (двойной запуск эффекта).
  useEffect(() => {
    if (!project) return;
    let off: (() => void) | undefined;
    let closed = false;
    let webview;
    try {
      webview = getCurrentWebview();
    } catch {
      return;
    }
    webview
      .onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") setDragOver(true);
        else if (p.type === "leave") setDragOver(false);
        else if (p.type === "drop") {
          setDragOver(false);
          void importFiles(p.paths);
        }
      })
      .then((f) => (closed ? f() : (off = f)))
      .catch(() => {});
    return () => {
      closed = true;
      off?.();
    };
  }, [project?.path, importFiles]);

  /** Итог правки проекта: ошибка — сообщение (новое состояние придет событием). */
  const edited = useCallback((r: { status: "ok" } | { status: "error"; error: AppError }) => r.status === "error" && fail(r.error), [fail]);

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
      { id: "closeProject", label: "cmd.closeProject", icon: FolderClosed, keys: "Ctrl+W", enabled: () => !!project, run: () => void closeProject() },
      { id: "undo", label: "cmd.undo", icon: Undo2, keys: "Ctrl+Z", enabled: () => !!project?.canUndo, run: () => void ipc.projectUndo().then(edited) },
      { id: "redo", label: "cmd.redo", icon: Redo2, keys: "Ctrl+Shift+Z", enabled: () => !!project?.canRedo, run: () => void ipc.projectRedo().then(edited) },
      { id: "chat", label: "cmd.chat", icon: MessageSquare, keys: "Ctrl+L", enabled: () => !!project, run: () => setChatOpen((o) => !o) },
      { id: "palette", label: "cmd.palette", icon: CommandIcon, keys: "Ctrl+Shift+P", run: () => setDialog("palette") },
      ...[0, 1, 2].map((i) => ({
        id: `tab${i + 1}`,
        label: `tabs.${(project ? PROJECT_TABS : HOME_TABS)[i]}`,
        keys: `Alt+${i + 1}`,
        run: () => setTab((project ? PROJECT_TABS : HOME_TABS)[i] ?? "home"),
      })),
      { id: "language", label: "cmd.language", icon: Languages, run: () => void toggleLanguage() },
      { id: "reload", label: "cmd.reload", icon: RotateCw, keys: "Ctrl+R", run: () => window.location.reload() },
      { id: "refreshChecks", label: "cmd.refreshChecks", icon: RefreshCw, keys: "F5", run: () => void check() },
      { id: "refreshLimits", label: "cmd.refreshLimits", icon: Gauge, run: () => void refreshLimits() },
      { id: "openLogs", label: "cmd.openLogs", icon: FileText, run: () => void appLogDir().then((d) => revealItemInDir(d)) },
      { id: "checkUpdates", label: "cmd.checkUpdates", icon: Download, run: () => void ipc.updateCheckNow() },
      { id: "installUpdate", label: "cmd.installUpdate", icon: Download, enabled: () => update?.phase === "ready", run: () => void ipc.updateInstall().then((r) => r.status === "error" && fail(r.error)) },
      { id: "quit", label: "cmd.quit", icon: LogOut, keys: "Alt+F4", run: () => void getCurrentWindow().close() },
    ],
    [check, closeProject, edited, fail, openFolder, project, refreshLimits, toggleLanguage, update?.phase],
  );
  const byId = useMemo(() => new Map(commandList.map((c) => [c.id, c])), [commandList]);

  const menus: MenuSpec[] = [
    { id: "file", label: "menu.file", items: ["newProject", "openProject", "closeProject", "-", "quit"] },
    { id: "edit", label: "menu.edit", items: ["undo", "redo", "-", "palette"] },
    { id: "view", label: "menu.view", items: ["tab1", "tab2", "tab3", "-", "language", "reload"] },
    { id: "window", label: "menu.window", items: ["chat", "-", "refreshChecks", "refreshLimits"] },
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
      <TitleBar menus={menus} commands={byId} subtitle={project?.project.name} />

      <div className="pagebar">
        <div className="tabs">
          {(project ? PROJECT_TABS : HOME_TABS).map((k) => (
            <button key={k} className={tab === k ? "on" : ""} onClick={() => setTab(k)}>
              {t(`tabs.${k}`)}
            </button>
          ))}
        </div>
        {project && (
          <div className="pb-tools">
            <button className="iconbtn" title={`${t("cmd.undo")} (Ctrl+Z)`} disabled={!project.canUndo} onClick={() => byId.get("undo")?.run()}>
              <Undo2 size={15} strokeWidth={1.75} />
            </button>
            <button className="iconbtn" title={`${t("cmd.redo")} (Ctrl+Shift+Z)`} disabled={!project.canRedo} onClick={() => byId.get("redo")?.run()}>
              <Redo2 size={15} strokeWidth={1.75} />
            </button>
          </div>
        )}
        <div className="pb-right">
          {!project && (
            <label className="search">
              <Search size={14} strokeWidth={1.75} />
              <input placeholder={t("search.projects")} value={query} onChange={(e) => setQuery(e.target.value)} />
            </label>
          )}
          {update?.phase === "ready" && update.version && (
            <button className="btn" title={update.notes ?? undefined} onClick={() => byId.get("installUpdate")?.run()}>
              <Download size={14} strokeWidth={1.75} />
              {t("update.install", { version: update.version })}
            </button>
          )}
          <LimitsIndicator limits={limits} refreshing={limitsBusy} onRefresh={() => void refreshLimits()} />
          {!project && (
            <button className="btn pri" onClick={() => setDialog("new")}>
              <Plus size={14} strokeWidth={2} />
              {t("home.newProject")}
            </button>
          )}
        </div>
      </div>

      {project ? (
        <div className="workspace">
          {tab === "prepare" ? (
            <Prepare
              view={project}
              onSequence={(sequence) => void ipc.projectSetSequence(sequence).then(edited)}
              onRename={(name) => void ipc.projectRename(name).then(edited)}
              onError={fail}
              onImport={(files) => void importFiles(files)}
              mediaKey={mediaKey}
              rulesKey={rulesKey}
            />
          ) : tab === "edit" ? (
            <EditPage view={project} onError={fail} onImport={(files) => void importFiles(files)} mediaKey={mediaKey} claudeBusy={claudeBusy} />
          ) : (
            <div className="soon">{t(`soon.${tab}`)}</div>
          )}
          {chatOpen && (
            <>
              <Splitter width={ws.chat} min={320} max={720} side="right" onChange={(v) => setWs("chat", v)} onReset={() => setWs("chat", 400)} />
              <ChatPanel projectPath={project.path} onError={fail} width={ws.chat} />
            </>
          )}
          {dragOver && (
            <div className="dropover">
              <span>{t("prepare.dropRelease")}</span>
            </div>
          )}
        </div>
      ) : tab === "projects" ? (
        <Projects projects={projects} query={query} onOpen={(p) => void openProject(p.path)} onForget={(p) => void ipc.projectForget(p.path).then(loadProjects)} />
      ) : tab === "home" ? (
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
        {errors > 0 && (
          <button
            className="ic errs end"
            title={t("status.errorsHint")}
            onClick={() => {
              if (!project) return;
              setChatOpen(true);
              void ipc.chatSend(t("status.errorsAsk"), "user").then((r) => r.status === "error" && fail(r.error));
            }}
          >
            <TriangleAlert size={12} strokeWidth={1.75} />
            {t("status.errors", { count: errors })}
          </button>
        )}
        <button className={`ic ${errors > 0 ? "" : "end"}`} onClick={() => void toggleLanguage()}>
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
            void openProject(p.path);
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
