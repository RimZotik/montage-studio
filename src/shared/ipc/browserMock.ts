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

const MOCK_CHAT = [
  { role: "user", text: "Предложи структуру ролика на 30 секунд", at: now - 300 },
  { role: "tool", text: "Read rules/brief.md", at: now - 298 },
  { role: "assistant", text: "По брифу — спокойный промо-ролик. Предлагаю три части:\n\n1. **Хук** — 3 с, крупный план функции.\n2. **Основная часть** — 22 с, три коротких сцены.\n3. **Финал** — 5 с, переход в начало.\n\n```studio-proposal\n{\"id\":\"p1\",\"kind\":\"plan\",\"title\":\"Структура на 30 секунд\",\"description\":\"Хук, три сцены, финал с переходом в начало.\",\"details\":[\"0–3 с: хук\",\"3–25 с: три сцены по ~7 с\",\"25–30 с: финал\"]}\n```\n\n```studio-ask\n{\"id\":\"q1\",\"question\":\"Какой темп?\",\"options\":[\"Спокойный\",\"Динамичный\",\"Как в правилах\"]}\n```", at: now - 290 },
  { role: "turn", text: JSON.stringify({ ok: true, interrupted: false, durationMs: 9200, inputTokens: 51000, outputTokens: 1300, costUsd: 0.003, denials: ["Write notes.txt"] }), at: now - 289 },
];

/** Имитация ответа: чтение файла, текст по частям, итог хода. */
function fakeReply() {
  const words = "Записал бриф в rules/brief.md: спокойный тон, 30 секунд, вертикальный формат.".split(" ");
  setTimeout(() => emit("chat-event", { kind: "tool", id: "t9", name: "Write", summary: "rules/brief.md" }), 200);
  setTimeout(() => emit("chat-event", { kind: "toolDone", id: "t9", error: false }), 500);
  words.forEach((w, i) => setTimeout(() => emit("chat-event", { kind: "delta", text: (i ? " " : "") + w }), 600 + i * 90));
  setTimeout(() => emit("chat-event", { kind: "turn", ok: true, interrupted: false, durationMs: 4100, inputTokens: 23000, outputTokens: 400, costUsd: 0.002, denials: [] }), 700 + words.length * 90);
}

export function installBrowserMock() {
  const w = window as unknown as Record<string, unknown>;
  if ("__TAURI_INTERNALS__" in w) return;
  let next = 1;
  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
    // Файлов проекта в браузере нет — пустая картинка вместо адреса файла.
    convertFileSrc: () => "data:image/gif;base64,R0lGODlhAQABAAAAACw=",
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
        const tr = (id: string, kind: string, name: string) => ({ id, kind, name, muted: false, locked: false });
        const clip = (id: string, track: string, kind: string, src: string | null, text: string | null, start: number, duration: number) => ({ id, track, kind, src, text, start, duration, offset: 0, props: { volume: 1, opacity: 1, scale: 1, x: 0, y: 0, fadeIn: 0, fadeOut: 0 } });
        mockProject = {
          path,
          project: { version: 1, name: path.split("\\").pop() ?? "Проект", series: null, sequence: { width: 1080, height: 1920, fps: 60, durationFrames: 0, background: "#000000" }, libraryRules: [] },
          tracks: [tr("v2", "video", "V2"), tr("v1", "video", "V1"), tr("s1", "captions", "S1"), tr("a1", "audio", "A1"), tr("a2", "audio", "A2"), tr("a3", "audio", "A3")],
          clips: [
            clip("c1", "v1", "media", "media/footage/screen-01.mp4", null, 0, 540),
            clip("c2", "v1", "media", "media/footage/screen-02.mp4", null, 540, 480),
            clip("c3", "v2", "media", "media/images/cover.png", null, 120, 240),
            clip("c4", "s1", "caption", null, "Смотри сюда", 60, 120),
            clip("c5", "s1", "caption", null, "Новая функция", 200, 150),
            clip("c6", "a1", "media", "media/voice/take-2.wav", null, 30, 1500),
            clip("c7", "a2", "media", "media/music/bed.mp3", null, 0, 1800),
            clip("c8", "a3", "media", "media/sfx/hit-low.wav", null, 540, 60),
          ],
          markers: [{ frame: 540, label: "удар" }],
          canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
        };
        return structuredClone(mockProject);
      }
      if (cmd === "project_set_sequence" && mockProject) {
        mockProject = { ...mockProject, project: { ...mockProject.project, sequence: args?.sequence }, canUndo: true };
        emit("project-changed", mockProject);
        return structuredClone(mockProject);
      }
      if (cmd === "media_list")
        return [
          ["footage", "screen-01.mp4", 42],
          ["footage", "screen-02.mp4", 18],
          ["footage", "broll-sunset.mov", 9],
          ["voice", "take-2.wav", 29],
          ["music", "bed.mp3", 130],
          ["sfx", "hit-low.wav", 1],
          ["images", "cover.png", null],
        ].map(([category, name, d]) => ({ path: `media/${category}/${name}`, name, category, size: 1000000, probe: d == null ? null : { durationS: d, width: 1080, height: 1920, audio: true, video: category === "footage" }, thumb: null }));
      if (cmd === "rules_read")
        return [
          { name: "brief", text: "# Бриф\n\nКороткий ролик о новой функции, 30 секунд, вертикальный.\nЗритель — пользователи, которые еще не пробовали функцию.\nТон — спокойный, уверенный.\n" },
          { name: "rules", text: "- Финал без «ссылка в профиле».\n- Цифры на экране — только реальные.\n" },
          { name: "style", text: "" },
          { name: "script", text: "" },
        ];
      if (cmd === "media_import") return { imported: 1, skipped: [{ name: "short01_v4.mp4", reason: "busy" }] };
      if (cmd === "chat_history") return MOCK_CHAT;
      if (cmd === "chat_busy") return false;
      if (cmd === "chat_send") {
        fakeReply();
        return null;
      }
      if (cmd === "timeline_apply" && mockProject) {
        // Упрощенно: перенос, обрезка, удаление, свойства — для просмотра интерфейса.
        for (const o of (args?.ops ?? []) as any[]) {
          const c = mockProject.clips.find((x: any) => x.id === o.id);
          if (o.op === "moveClip" && c) Object.assign(c, { start: o.start, track: o.track ?? c.track });
          if (o.op === "trimClip" && c) Object.assign(c, { start: o.start ?? c.start, duration: o.duration ?? c.duration });
          if (o.op === "deleteClips") mockProject.clips = mockProject.clips.filter((x: any) => !o.ids.includes(x.id));
          if (o.op === "setProps" && c) c.props = o.props;
          if (o.op === "addClip") mockProject.clips.push({ ...o, id: `c${Date.now()}`, offset: 0, props: { volume: 1, opacity: 1, scale: 1, x: 0, y: 0, fadeIn: 0, fadeOut: 0 } });
        }
        mockProject = { ...mockProject, clips: [...mockProject.clips], canUndo: true };
        emit("project-changed", mockProject);
        return [];
      }
      if (cmd === "project_close") {
        mockProject = null;
        return null;
      }
      if (cmd === "settings_update") return args?.value;
      // `#install` в адресе — сразу показать ход установки ffmpeg (для снимков).
      if (cmd === "system_check" && location.hash === "#install") setTimeout(() => fakeInstall("ffmpeg"), 300);
      if (cmd === "system_check" && location.hash === "#edit") setTimeout(() => { (document.querySelector(".pr .th") as HTMLButtonElement | null)?.click(); setTimeout(() => (document.querySelectorAll(".tabs button")[1] as HTMLElement | undefined)?.click(), 400); setTimeout(() => (document.querySelector(".clip.video") as HTMLElement | null)?.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0 })), 900); }, 300);
      if (cmd === "system_check" && location.hash === "#preview") setTimeout(() => { (document.querySelector(".pr .th") as HTMLButtonElement | null)?.click(); setTimeout(() => (document.querySelector(".mc") as HTMLElement | null)?.click(), 400); }, 300);
      if (cmd === "system_check" && location.hash === "#project") setTimeout(() => (document.querySelector(".pr .th") as HTMLButtonElement | null)?.click(), 300);
      if (cmd === "claude_limits_refresh") return DATA.claude_limits_last;
      if (cmd in DATA) return structuredClone(DATA[cmd]);
      return null;
    },
  };
}
