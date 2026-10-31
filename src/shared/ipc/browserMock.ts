// Просмотр интерфейса в обычном браузере (`npm run dev` без Tauri): команды ядра отвечают
// образцами данных. Только для разработки — в сборку приложения не попадает (main.tsx подключает
// его лишь в режиме разработки и без Tauri).

const now = Math.floor(Date.now() / 1000);
const day = 86400;

const DATA: Record<string, unknown> = {
  settings_get: { version: 1, language: "", projectsDir: "D:\\Видео\\Проекты" },
  settings_update: null,
  system_check: {
    tools: [
      { id: "claude", found: true, version: "2.1.316", path: "C:\\Users\\user\\.local\\bin\\claude.exe", source: "system", error: null, component: null, latest: null, outdated: false, downloadMb: null },
      { id: "node", found: true, version: "24.18.0", path: "C:\\Program Files\\nodejs\\node.exe", source: "system", error: null, component: "node", latest: "24.22.2", outdated: false, downloadMb: 37 },
      { id: "ffmpeg", found: false, version: null, path: null, source: null, error: null, component: "ffmpeg", latest: "9.0.2", outdated: false, downloadMb: 96 },
      { id: "ffprobe", found: false, version: null, path: null, source: null, error: null, component: "ffmpeg", latest: "9.0.2", outdated: false, downloadMb: 96 },
    ],
    hardware: { cpu: "AMD Ryzen 7 7800X3D 8-Core Processor", threads: 16, memoryGb: 32, gpus: ["NVIDIA GeForce RTX 4070"] },
  },
  projects_recent: [
    ["Промо, октябрь", 1080, 1920, 60, now - 3600],
    ["Обзор функций", 1920, 1080, 30, now - day],
    ["Тизер", 1080, 1920, 60, now - 4 * day],
    ["Инструкция", 1920, 1080, 30, now - 7 * day],
    ["Анонс", 1080, 1080, 30, now - 12 * day],
    ["Старый черновик", 1080, 1920, 60, now - 40 * day],
  ].map(([name, width, height, fps, at], i) => ({ path: `D:\\Видео\\Проекты\\${name}`, name, width, height, fps, createdAt: at, openedAt: at, exists: i !== 5 })),
  claude_limits_last: {
    at: now - 600,
    fiveHour: { utilization: 0.33, resetsAt: now + 2 * 3600, status: "allowed" },
    sevenDay: { utilization: 0.78, resetsAt: now + 5 * day, status: "allowed_warning" },
  },
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let mockProject: any = null;
const callbacks = new Map<number, (payload: unknown) => void>();
const listeners = new Map<string, number[]>();

function emit(event: string, payload: unknown) {
  for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
}

/** Имитация установки компонента: загрузка по шагам, затем проверка. */
function fakeInstall(id: string) {
  const total = id === "node" ? 37_754_834 : 99_899_672;
  let received = 0;
  const timer = setInterval(() => {
    received = Math.min(total, received + total / 25);
    emit("component-progress", { id, phase: received < total ? "download" : "verify", received, total, error: null });
    if (received >= total) clearInterval(timer);
  }, 200);
}

export function installBrowserMock() {
  const w = window as unknown as Record<string, unknown>;
  if ("__TAURI_INTERNALS__" in w) return;
  let next = 1;
  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback: (cb: (payload: unknown) => void) => {
      const id = next++;
      callbacks.set(id, cb);
      return id;
    },
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "plugin:event|listen") {
        const event = String(args?.event);
        listeners.set(event, [...(listeners.get(event) ?? []), Number(args?.handler)]);
        return Number(args?.handler);
      }
      if (cmd === "component_install") {
        fakeInstall(String(args?.id));
        return null;
      }
      if (cmd === "update_status") return { version: "0.2.0", notes: null, phase: "ready", progress: 1 };
      // Проект: открытие, правки последовательности и имени с отменой — в памяти.
      if (cmd === "project_load") {
        const path = String(args?.path);
        mockProject = { path, project: { version: 1, name: path.split("\\").pop() ?? "Проект", series: null, sequence: { width: 1080, height: 1920, fps: 60, durationFrames: 0, background: "#000000" }, libraryRules: [] }, tracks: 0, clips: 0, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null };
        return structuredClone(mockProject);
      }
      if (cmd === "project_set_sequence" && mockProject) {
        mockProject = { ...mockProject, project: { ...mockProject.project, sequence: args?.sequence }, canUndo: true };
        emit("project-changed", mockProject);
        return structuredClone(mockProject);
      }
      if (cmd === "project_close") {
        mockProject = null;
        return null;
      }
      if (cmd === "settings_update") return args?.value;
      // `#install` в адресе — сразу показать ход установки ffmpeg (для снимков).
      if (cmd === "system_check" && location.hash === "#install") setTimeout(() => fakeInstall("ffmpeg"), 300);
      if (cmd === "system_check" && location.hash === "#project") setTimeout(() => (document.querySelector(".pr .th") as HTMLButtonElement | null)?.click(), 300);
      if (cmd === "claude_limits_refresh") return DATA.claude_limits_last;
      if (cmd in DATA) return structuredClone(DATA[cmd]);
      return null;
    },
  };
}
