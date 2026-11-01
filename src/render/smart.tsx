import * as Lucide from "lucide-react";
import * as React from "react";
import * as Remotion from "remotion";
import { transform } from "sucrase";

// Умные клипы (docs/smart-clips.md): компоненты из папки `smart/` проекта. Исходник собирается прямо
// здесь, поэтому монитор и рендер исполняют одно и то же. Запуск только для проекта, которому доверяют:
// исходники приходят из ядра, а оно не отдает их без доверия.

export type SmartSources = Record<string, string>;
type Module = { exports: Record<string, unknown> };

const EXT = ["", ".tsx", ".ts", ".jsx", ".js", ".json", "/index.tsx", "/index.ts"];

function join(from: string, spec: string): string {
  const parts = from.split("/").slice(0, -1);
  for (const p of spec.split("/")) {
    if (p === "..") parts.pop();
    else if (p !== ".") parts.push(p);
  }
  return parts.join("/");
}

/** Загрузчик модулей проекта: `./lib/x` — из `smart/`, остальное — разрешенные библиотеки. `staticFile`
 *  указывает на файлы проекта: `media/...` как есть, короткий путь — в `media/assets/`. */
export function smartLoader(sources: SmartSources, srcOf: (path: string) => string) {
  const staticFile = (p: string) => {
    const clean = p.replace(/^\/+/, "");
    return srcOf(clean.startsWith("media/") ? clean : `media/assets/${clean}`);
  };
  const remotion = { ...Remotion, staticFile };
  const cache = new Map<string, Module>();
  const load = (path: string): Module => {
    const known = cache.get(path);
    if (known) return known;
    const module: Module = { exports: {} };
    cache.set(path, module);
    const text = sources[path] ?? "";
    if (path.endsWith(".json")) {
      module.exports = JSON.parse(text) as Record<string, unknown>;
      return module;
    }
    const code = transform(text, { transforms: ["typescript", "jsx", "imports"], jsxRuntime: "classic", production: true, filePath: path }).code;
    const require = (spec: string) => {
      if (spec === "react") return React;
      if (spec === "remotion") return remotion;
      if (spec === "lucide-react") return Lucide;
      if (spec.endsWith(".css")) return {};
      if (!spec.startsWith(".")) throw new Error(`${path}: "${spec}" is not available in smart clips`);
      const base = join(path, spec);
      const hit = EXT.map((x) => base + x).find((x) => x in sources);
      if (!hit) throw new Error(`${path}: "${spec}" not found`);
      return load(hit).exports;
    };
    // Запуск кода проекта, которому пользователь доверяет: это и есть назначение умных клипов.
    new Function("require", "module", "exports", "React", code)(require, module, module.exports, React);
    return module;
  };
  return load;
}

type Boundary = { error: Error | null };

/** Ошибка сцены не роняет монитор и рендер: видна прямо в кадре, Claude увидит ее на проверочном кадре. */
class SmartBoundary extends React.Component<{ name: string; children: React.ReactNode }, Boundary> {
  override state: Boundary = { error: null };
  static getDerivedStateFromError(error: Error): Boundary {
    return { error };
  }
  override render() {
    if (!this.state.error) return this.props.children;
    return <SmartError name={this.props.name} message={this.state.error.message} />;
  }
}

function SmartError({ name, message }: { name: string; message: string }) {
  return (
    <Remotion.AbsoluteFill style={{ background: "#3a0d0d", color: "#ffd9d9", padding: 40, fontSize: 32, fontFamily: "monospace", whiteSpace: "pre-wrap" }}>
      {`${name}\n${message}`}
    </Remotion.AbsoluteFill>
  );
}

/** Клип: `src` — файл компонента, `text` — его данные в JSON. Компонент — экспорт по умолчанию, иначе
 *  первый экспорт-функция. */
export function SmartClip({ src, data, sources, srcOf }: { src: string; data: string | null; sources: SmartSources; srcOf: (path: string) => string }) {
  const built = React.useMemo(() => {
    try {
      const m = smartLoader(sources, srcOf)(src).exports;
      const C = (m.default ?? Object.values(m).find((v) => typeof v === "function")) as React.ComponentType<Record<string, unknown>> | undefined;
      if (!C) return { error: `${src}: no component exported` };
      return { C };
    } catch (e) {
      return { error: e instanceof Error ? e.message : String(e) };
    }
  }, [src, sources, srcOf]);
  const props = React.useMemo(() => {
    try {
      return data ? (JSON.parse(data) as Record<string, unknown>) : {};
    } catch {
      return {};
    }
  }, [data]);
  if (built.error || !built.C) return <SmartError name={src} message={built.error ?? ""} />;
  return (
    <SmartBoundary name={src}>
      <built.C {...props} />
    </SmartBoundary>
  );
}
