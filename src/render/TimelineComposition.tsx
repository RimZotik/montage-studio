import { AbsoluteFill, Html5Audio, Html5Video, Img, Sequence, interpolate, useCurrentFrame } from "remotion";
import type { Clip, Sequence as Seq, Track } from "@/shared/ipc/bindings";

// Композиция «таймлайн» (docs/architecture.md): из данных проекта собирается кадр — тот же код
// играет в предпросмотре (Remotion Player) и позже рендерит видео, поэтому картинка совпадает.

export type CompositionProps = {
  sequence: Seq;
  tracks: Track[];
  clips: Clip[];
  /** Путь внутри проекта → адрес файла для плеера. */
  srcOf: (path: string) => string;
};

const IMAGE = /\.(png|jpe?g|webp|gif|svg|bmp)$/i;
/** Свойства клипа со значениями по умолчанию (в файле поля могут отсутствовать). */
export function propsOf(clip: Clip) {
  const p = clip.props;
  return { volume: p?.volume ?? 1, opacity: p?.opacity ?? 1, scale: p?.scale ?? 1, x: p?.x ?? 0, y: p?.y ?? 0, fadeIn: p?.fadeIn ?? 0, fadeOut: p?.fadeOut ?? 0 };
}

/** Плавное появление и исчезновение (0–1) по кадру внутри клипа. */
function fade(frame: number, clip: Clip): number {
  const { fadeIn, fadeOut } = propsOf(clip);
  let k = 1;
  if (fadeIn > 0) k = Math.min(k, interpolate(frame, [0, fadeIn], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }));
  if (fadeOut > 0) k = Math.min(k, interpolate(frame, [clip.duration - fadeOut, clip.duration], [1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }));
  return k;
}

function VisualClip({ clip, srcOf }: { clip: Clip; srcOf: (p: string) => string }) {
  const frame = useCurrentFrame();
  const p = propsOf(clip);
  const style = { opacity: p.opacity * fade(frame, clip), transform: `translate(${p.x}px, ${p.y}px) scale(${p.scale})` };
  if (clip.kind === "solid") return <AbsoluteFill style={{ ...style, background: clip.text ?? "#000000" }} />;
  const src = srcOf(clip.src ?? "");
  return (
    <AbsoluteFill style={style}>
      {IMAGE.test(clip.src ?? "") ? (
        <Img src={src} style={{ width: "100%", height: "100%", objectFit: "cover" }} />
      ) : (
        <Html5Video src={src} trimBefore={clip.offset} volume={(f: number) => p.volume * fade(f, clip)} style={{ width: "100%", height: "100%", objectFit: "cover" }} />
      )}
    </AbsoluteFill>
  );
}

function CaptionClip({ clip }: { clip: Clip }) {
  const frame = useCurrentFrame();
  return (
    <AbsoluteFill style={{ justifyContent: "flex-end", alignItems: "center", paddingBottom: "34%", opacity: fade(frame, clip) }}>
      <div style={{ fontFamily: "Geist Sans, system-ui, sans-serif", fontWeight: 700, fontSize: 64, color: "#fff", textAlign: "center", lineHeight: 1.1, padding: "0 8%", textShadow: "0 3px 0 #000, 0 0 18px rgba(0,0,0,.55)", WebkitTextStroke: "2px #000" }}>
        {clip.text}
      </div>
    </AbsoluteFill>
  );
}

export function TimelineComposition({ sequence, tracks, clips, srcOf }: CompositionProps) {
  // Дорожки идут сверху вниз (V2 выше V1) — рисовать снизу вверх.
  const visual = [...tracks].reverse().filter((t) => t.kind !== "audio" && !t.muted);
  const audio = tracks.filter((t) => t.kind === "audio" && !t.muted);
  return (
    <AbsoluteFill style={{ background: sequence.background ?? "#000000" }}>
      {visual.map((t) =>
        clips
          .filter((c) => c.track === t.id)
          .map((c) => (
            <Sequence key={c.id} from={c.start} durationInFrames={c.duration} layout="none">
              <AbsoluteFill>{c.kind === "caption" ? <CaptionClip clip={c} /> : <VisualClip clip={c} srcOf={srcOf} />}</AbsoluteFill>
            </Sequence>
          )),
      )}
      {audio.map((t) =>
        clips
          .filter((c) => c.track === t.id && c.src)
          .map((c) => (
            <Sequence key={c.id} from={c.start} durationInFrames={c.duration} layout="none">
              <Html5Audio src={srcOf(c.src ?? "")} trimBefore={c.offset} volume={(f: number) => propsOf(c).volume * fade(f, c)} />
            </Sequence>
          )),
      )}
    </AbsoluteFill>
  );
}
