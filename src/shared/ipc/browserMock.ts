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
      { id: "claude", found: true, version: "2.1.316", path: "C:\\Users\\user\\.local\\bin\\claude.exe", source: "system", error: null },
      { id: "node", found: true, version: "24.18.0", path: "C:\\Program Files\\nodejs\\node.exe", source: "system", error: null },
      { id: "ffmpeg", found: false, version: null, path: null, source: null, error: null },
      { id: "ffprobe", found: false, version: null, path: null, source: null, error: null },
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

export function installBrowserMock() {
  const w = window as unknown as Record<string, unknown>;
  if ("__TAURI_INTERNALS__" in w) return;
  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    transformCallback: () => 0,
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "settings_update") return args?.value;
      if (cmd === "claude_limits_refresh") return DATA.claude_limits_last;
      if (cmd in DATA) return structuredClone(DATA[cmd]);
      return null;
    },
  };
}
