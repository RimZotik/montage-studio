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

/** Части одного файла встык (после разреза), продолжающие друг друга в файле и с одинаковыми
 *  свойствами, играются одним клипом — иначе на стыке новый проигрыватель дает разрыв звука и
 *  картинки. */
export function joinRuns(clips: Clip[]): Clip[] {
  const out: Clip[] = [];
  for (const c of [...clips].sort((a, b) => a.track.localeCompare(b.track) || a.start - b.start)) {
    const prev = out[out.length - 1];
    const same = (a: Clip, b: Clip) => {
      const pa = propsOf(a);
      const pb = propsOf(b);
      return pa.volume === pb.volume && pa.opacity === pb.opacity && pa.scale === pb.scale && pa.x === pb.x && pa.y === pb.y;
    };
    if (
      prev &&
      prev.kind === "media" &&
      c.kind === "media" &&
      prev.track === c.track &&
      prev.src === c.src &&
      prev.start + prev.duration === c.start &&
      (prev.offset ?? 0) + prev.duration === (c.offset ?? 0) &&
      propsOf(prev).fadeOut === 0 &&
      propsOf(c).fadeIn === 0 &&
      same(prev, c)
    ) {
      out[out.length - 1] = { ...prev, duration: prev.duration + c.duration, props: { ...propsOf(prev), fadeOut: propsOf(c).fadeOut } };
    } else out.push(c);
  }
  return out;
}

export function TimelineComposition({ sequence, tracks, clips: raw, srcOf }: CompositionProps) {
  const clips = joinRuns(raw);
  // Следующий клип подгружается заранее, чтобы на стыке не было паузы.
  const premount = sequence.fps;
  // Дорожки идут сверху вниз (V2 выше V1) — рисовать снизу вверх.
  const visual = [...tracks].reverse().filter((t) => t.kind !== "audio" && !t.muted);
  const audio = tracks.filter((t) => t.kind === "audio" && !t.muted);
  return (
    <AbsoluteFill style={{ background: sequence.background ?? "#000000" }}>
      {visual.map((t) =>
        clips
          .filter((c) => c.track === t.id)
          .map((c) => (
            <Sequence key={c.id} from={c.start} durationInFrames={c.duration} premountFor={premount}>
              <AbsoluteFill>{c.kind === "caption" ? <CaptionClip clip={c} /> : <VisualClip clip={c} srcOf={srcOf} />}</AbsoluteFill>
            </Sequence>
          )),
      )}
      {audio.map((t) =>
        clips
          .filter((c) => c.track === t.id && c.src)
          .map((c) => (
            <Sequence key={c.id} from={c.start} durationInFrames={c.duration} premountFor={premount}>
              <Html5Audio src={srcOf(c.src ?? "")} trimBefore={c.offset} volume={(f: number) => propsOf(c).volume * fade(f, c)} />
            </Sequence>
          )),
      )}
    </AbsoluteFill>
  );
}
