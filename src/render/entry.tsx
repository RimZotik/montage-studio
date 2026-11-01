import "@fontsource/geist-sans/700.css";
import { Composition, registerRoot } from "remotion";
import type { ComponentType } from "react";
import type { Clip, Sequence, Track } from "@/shared/ipc/bindings";
import { TimelineComposition } from "./TimelineComposition";

// Вход сборки для движка рендера (`scripts/build-render-bundle.mjs`): та же композиция, что в
// мониторе. Файлы медиа отдает локальный сервер задания (`baseUrl`).

type Props = { sequence: Sequence; tracks: Track[]; clips: Clip[]; baseUrl: string };

function Timeline({ sequence, tracks, clips, baseUrl }: Props) {
  return <TimelineComposition sequence={sequence} tracks={tracks} clips={clips} srcOf={(p) => `${baseUrl}/${p.split("/").map(encodeURIComponent).join("/")}`} />;
}

function Root() {
  const defaults: Props = { sequence: { width: 1080, height: 1920, fps: 30, durationFrames: 0, background: "#000000" }, tracks: [], clips: [], baseUrl: "" };
  return (
    <Composition
      id="timeline"
      component={Timeline as ComponentType<Record<string, unknown>>}
      width={1080}
      height={1920}
      fps={30}
      durationInFrames={1}
      defaultProps={defaults as unknown as Record<string, unknown>}
      calculateMetadata={({ props }) => {
        const p = props as unknown as Props;
        const end = Math.max(1, ...p.clips.map((c) => c.start + c.duration));
        return { width: p.sequence.width, height: p.sequence.height, fps: p.sequence.fps, durationInFrames: end };
      }}
    />
  );
}

registerRoot(Root);
