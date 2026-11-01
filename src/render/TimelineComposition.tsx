import { AbsoluteFill, Html5Audio, Html5Video, Img, OffthreadVideo, Sequence, interpolate, useCurrentFrame, useRemotionEnvironment, useVideoConfig } from "remotion";
import type { Clip, Sequence as Seq, Track } from "@/shared/ipc/bindings";
import { effectsAt, volumeAt } from "./effects";

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
  return { volume: p?.volume ?? 1, opacity: p?.opacity ?? 1, scale: p?.scale ?? 1, x: p?.x ?? 0, y: p?.y ?? 0, fadeIn: p?.fadeIn ?? 0, fadeOut: p?.fadeOut ?? 0, speed: p?.speed ?? 1, volumeKeys: p?.volumeKeys ?? [] };
}

/** Плавное появление и исчезновение (0–1) по кадру внутри клипа. */
function fade(frame: number, clip: Clip): number {
  const { fadeIn, fadeOut } = propsOf(clip);
  let k = 1;
  if (fadeIn > 0) k = Math.min(k, interpolate(frame, [0, fadeIn], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }));
  if (fadeOut > 0) k = Math.min(k, interpolate(frame, [clip.duration - fadeOut, clip.duration], [1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }));
  return k;
}

/** Громкость части в кадре `abs` таймлайна: свойство, плавность, огибающая (в кадрах файла),
 *  эффекты, дорожка. */
function loudnessAt(q: Clip, abs: number, fps: number, trackVolume: number): number {
  const inClip = abs - q.start;
  return propsOf(q).volume * fade(inClip, q) * volumeAt(propsOf(q).volumeKeys as [number, number][], (q.offset ?? 0) + inClip * propsOf(q).speed) * effectsAt(q.effects, inClip, q.duration, fps).volume * trackVolume;
}

/** Непрерывный кусок одного файла: части встык, продолжающие друг друга в файле (после
 *  разреза). Играется одним проигрывателем — без пауз и потерянных кадров на стыках; свойства
 *  каждой части применяются по кадрам. `z` — выше рисуется позже. */
export type Run = { id: string; track: string; src: string; start: number; duration: number; offset: number; parts: Clip[]; z: number };

export function runsOf(clips: Clip[]): Run[] {
  const z = new Map(clips.map((c, i) => [c.id, i]));
  const out: Run[] = [];
  const media = clips.filter((c) => c.kind === "media" && c.src).sort((a, b) => a.track.localeCompare(b.track) || a.start - b.start);
  for (const c of media) {
    const prev = out[out.length - 1];
    const last = prev?.parts[prev.parts.length - 1];
    if (prev && last && prev.track === c.track && prev.src === c.src && prev.start + prev.duration === c.start && propsOf(last).speed === propsOf(c).speed && Math.round((last.offset ?? 0) + last.duration * propsOf(last).speed) === (c.offset ?? 0)) {
      prev.duration += c.duration;
      prev.parts.push(c);
      prev.z = Math.max(prev.z, z.get(c.id) ?? 0);
    } else out.push({ id: c.id, track: c.track, src: c.src ?? "", start: c.start, duration: c.duration, offset: c.offset ?? 0, parts: [c], z: z.get(c.id) ?? 0 });
  }
  return out;
}

/** Часть куска под кадром `f` (от начала куска). */
function partAt(run: Run, f: number): Clip {
  const abs = run.start + f;
  return run.parts.find((p) => abs >= p.start && abs < p.start + p.duration) ?? (run.parts[run.parts.length - 1] as Clip);
}

/** Закрыт ли кадр (абсолютный) куском выше на той же дорожке. */
function covered(abs: number, run: Run, same: Run[]): boolean {
  return same.some((h) => h.z > run.z && abs >= h.start && abs < h.start + h.duration);
}

function VisualRun({ run, srcOf, muted, trackVolume }: { run: Run; srcOf: (p: string) => string; muted: boolean; trackVolume: number }) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const part = partAt(run, frame);
  const p = propsOf(part);
  const inClip = run.start + frame - part.start;
  const fx = effectsAt(part.effects, inClip, part.duration, fps);
  const style = { opacity: p.opacity * fx.opacity * fade(inClip, part), transform: `translate(${p.x + fx.x}px, ${p.y + fx.y}px) scale(${p.scale * fx.scale}) rotate(${fx.rotate}deg)`, filter: fx.filter || undefined };
  const src = srcOf(run.src);
  // При рендере кадр берется точно из файла; в мониторе — обычное видео браузера, без пауз.
  const Clip = useRemotionEnvironment().isRendering ? OffthreadVideo : Html5Video;
  return (
    <AbsoluteFill style={style}>
      {IMAGE.test(run.src) ? (
        <Img src={src} style={{ width: "100%", height: "100%", objectFit: "cover" }} />
      ) : (
        <Clip
          src={src}
          trimBefore={run.offset}
          playbackRate={propsOf(run.parts[0] as Clip).speed}
          volume={(f: number) => {
            if (muted) return 0;
            return loudnessAt(partAt(run, f), run.start + f, fps, trackVolume);
          }}
          style={{ width: "100%", height: "100%", objectFit: "cover" }}
        />
      )}
    </AbsoluteFill>
  );
}

function SolidClip({ clip }: { clip: Clip }) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const p = propsOf(clip);
  const fx = effectsAt(clip.effects, frame, clip.duration, fps);
  return <AbsoluteFill style={{ background: clip.text ?? "#000000", opacity: p.opacity * fx.opacity * fade(frame, clip), transform: `translate(${p.x + fx.x}px, ${p.y + fx.y}px) scale(${p.scale * fx.scale}) rotate(${fx.rotate}deg)`, filter: fx.filter || undefined }} />;
}

/** Субтитр: нижняя треть по умолчанию; `props.y` сдвигает (минус — вверх), `scale` — размер. */
function CaptionClip({ clip }: { clip: Clip }) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const p = propsOf(clip);
  const fx = effectsAt(clip.effects, frame, clip.duration, fps);
  const plate = fx.background && !/^#[0-9a-f]{6}00$/i.test(fx.background) ? fx.background : undefined;
  return (
    <AbsoluteFill style={{ justifyContent: "flex-end", alignItems: "center", paddingBottom: "34%", opacity: p.opacity * fx.opacity * fade(frame, clip), transform: `translate(${p.x + fx.x}px, ${p.y + fx.y}px) scale(${p.scale * fx.scale}) rotate(${fx.rotate}deg)`, filter: fx.filter || undefined }}>
      <div style={{ fontFamily: fx.font ?? "Geist Sans, system-ui, sans-serif", fontWeight: fx.weight ?? 700, fontSize: fx.size ?? 64, color: fx.color ?? "#fff", textAlign: "center", textTransform: fx.uppercase ? "uppercase" : undefined, lineHeight: 1.1, margin: "0 8%", padding: plate ? "6px 20px" : 0, borderRadius: 12, background: plate, textShadow: plate ? undefined : "0 3px 0 #000, 0 0 18px rgba(0,0,0,.55)", WebkitTextStroke: plate || fx.stroke === 0 ? undefined : `${fx.stroke ?? 2}px #000` }}>
        {clip.text}
      </div>
    </AbsoluteFill>
  );
}

export function TimelineComposition({ sequence, tracks, clips, srcOf }: CompositionProps) {
  const { fps } = useVideoConfig();
  const runs = runsOf(clips);
  const z = new Map(clips.map((c, i) => [c.id, i]));
  // Следующий кусок подгружается заранее, чтобы на стыке не было паузы.
  const premount = sequence.fps;
  const audioOn = new Set(tracks.filter((t) => t.kind === "audio" && !t.muted).map((t) => t.id));
  // Связки, у которых есть звук на включенной звуковой дорожке: картинка играет без звука.
  const soundLinks = new Set(clips.filter((c) => c.link && audioOn.has(c.track)).map((c) => c.link));
  // Дорожки идут сверху вниз (V2 выше V1) — рисовать снизу вверх; субтитры всегда поверх видео;
  // внутри дорожки — по z.
  const shown = [...tracks].reverse().filter((t) => !t.muted);
  const visual = [...shown.filter((t) => t.kind === "video"), ...shown.filter((t) => t.kind === "captions")];
  return (
    <AbsoluteFill style={{ background: sequence.background ?? "#000000" }}>
      {visual.map((t) => {
        const items = [
          ...runs
            .filter((r) => r.track === t.id)
            .map((r) => ({
              z: r.z,
              el: (
                <Sequence key={r.id} from={r.start} durationInFrames={r.duration} premountFor={premount}>
                  <VisualRun run={r} srcOf={srcOf} trackVolume={t.volume ?? 1} muted={r.parts.some((p) => !!p.link && soundLinks.has(p.link))} />
                </Sequence>
              ),
            })),
          ...clips
            .filter((c) => c.track === t.id && c.kind !== "media")
            .map((c) => ({
              z: z.get(c.id) ?? 0,
              el: (
                <Sequence key={c.id} from={c.start} durationInFrames={c.duration}>
                  {c.kind === "caption" ? <CaptionClip clip={c} /> : <SolidClip clip={c} />}
                </Sequence>
              ),
            })),
        ];
        return items.sort((a, b) => a.z - b.z).map((i) => i.el);
      })}
      {tracks
        .filter((t) => audioOn.has(t.id))
        .map((t) => {
          const own = runs.filter((r) => r.track === t.id);
          return own.map((r) => (
            <Sequence key={r.id} from={r.start} durationInFrames={r.duration} premountFor={premount}>
              <Html5Audio
                src={srcOf(r.src)}
                trimBefore={r.offset}
                playbackRate={propsOf(r.parts[0] as Clip).speed}
                volume={(f: number) => {
                  // Звук нижнего клипа под верхним на той же дорожке не слышен.
                  if (covered(r.start + f, r, own)) return 0;
                  return loudnessAt(partAt(r, f), r.start + f, fps, t.volume ?? 1);
                }}
              />
            </Sequence>
          ));
        })}
    </AbsoluteFill>
  );
}
