import { AudioLines, Blend, Clapperboard, Type, Wand2 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AppliedEffect, Item } from "@/shared/ipc/bindings";
import { effectsAt } from "@/render/effects";
import { defaultsOf } from "./ControlPanel";

export const TARGET_ICON: Record<string, LucideIcon> = { video: Clapperboard, audio: AudioLines, text: Type, sequence: Wand2, transition: Blend };

/** Значки назначения элемента библиотеки. */
export function TargetIcons({ targets }: { targets: string[] }) {
  const { t } = useTranslation();
  return (
    <span className="fxtargets">
      {targets.map((x) => {
        const Icon = TARGET_ICON[x];
        return Icon ? (
          <span key={x} title={t(`library.target.${x}`)}>
            <Icon size={12} strokeWidth={1.75} />
          </span>
        ) : null;
      })}
    </span>
  );
}

const FPS = 30;
const LEN = 90;
/** Обычная речь и музыка: тихие и громкие места вперемешку. */
const SPEECH = Array.from({ length: 48 }, (_, i) => Math.min(1, 0.18 + 0.72 * Math.abs(Math.sin(i * 0.61) * Math.cos(i * 0.23))));

function Bars({ values, tone }: { values: number[]; tone: string }) {
  const top = Math.max(1e-6, ...values, 1);
  const width = 100 / Math.max(1, values.length);
  return (
    <svg className="pvbars" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden>
      {values.map((v, i) => {
        const h = Math.max(0.5, (v / top) * 38);
        return <rect key={i} x={i * width + width * 0.12} y={20 - h / 2} width={width * 0.76} height={h} rx={0.4} fill={tone} />;
      })}
    </svg>
  );
}

function Blocks({ values, tone }: { values: number[]; tone: string }) {
  const total = values.reduce((a, b) => a + b, 0) || 1;
  return (
    <div className="pvblocks">
      {values.map((v, i) => (
        <i key={i} style={{ flexGrow: v / total, background: tone, opacity: i % 2 ? 0.6 : 1 }} />
      ))}
    </div>
  );
}

function fxOf(item: Item, values?: Record<string, unknown>): AppliedEffect {
  return { id: "fx1", effect: item.id, name: item.name, enabled: true, params: { ...defaultsOf(item.controls), ...values }, controls: item.controls, spec: item.spec };
}

/** Сцена-образец: на ней видно и цвет, и движение. */
function Scene({ text }: { text: boolean }) {
  return (
    <div className="pvscene">
      <i className="sun" />
      <i className="hill" />
      {!text && <i className="hill two" />}
      {text && <b>Aa</b>}
    </div>
  );
}

function EffectSplit({ item, values, live }: { item: Item; values?: Record<string, unknown>; live: boolean }) {
  const { t } = useTranslation();
  const [frame, setFrame] = useState(LEN / 2);
  const [split, setSplit] = useState(50);
  const [hover, setHover] = useState(false);
  const fx = useMemo(() => [fxOf(item, values)], [item, values]);
  const targets = item.targets ?? [];
  const text = targets.includes("text") && !targets.includes("video");
  const moving = live || hover;
  useEffect(() => {
    if (!moving) return;
    const t0 = performance.now();
    let raf = requestAnimationFrame(function tick(now) {
      setFrame(Math.floor(((now - t0) / 1000) * FPS) % LEN);
      raf = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(raf);
  }, [moving]);
  const s = effectsAt(fx, frame, LEN, FPS);
  const style = { opacity: s.opacity, transform: `translate(${s.x / 8}px, ${s.y / 8}px) scale(${s.scale}) rotate(${s.rotate}deg)`, filter: s.filter || undefined };
  return (
    <div
      className="pv split"
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      onPointerMove={(e) => {
        if (!live) return;
        const r = e.currentTarget.getBoundingClientRect();
        setSplit(Math.max(4, Math.min(96, ((e.clientX - r.left) / r.width) * 100)));
      }}
    >
      <div className="pvlayer">
        <Scene text={text} />
      </div>
      <div className="pvlayer" style={{ clipPath: `inset(0 0 0 ${split}%)` }}>
        <div className="pvmoved" style={style}>
          <div className={text ? "pvtext" : "pvfill"} style={text ? { color: s.color, background: s.background } : undefined}>
            <Scene text={text} />
          </div>
        </div>
      </div>
      <i className="pvline" style={{ left: `${split}%` }} />
      <span className="pvtag l">{t("library.before")}</span>
      <span className="pvtag r">{t("library.after")}</span>
    </div>
  );
}

/** Предпросмотр на карточке: левая половина — как было, правая — как станет. Эффект показывается
 *  вживую по своему описанию; инструмент и звук — данными `preview` (волны, куски, кадры). Без
 *  подходящих данных ничего не рисует. */
export function PresetPreview({ item, values, live = false }: { item: Item; values?: Record<string, unknown>; live?: boolean }) {
  const { t } = useTranslation();
  const box = useRef<HTMLDivElement>(null);
  const p = item.preview as { type?: string; before?: unknown; after?: unknown } | null | undefined;
  const targets = item.targets ?? [];
  if (p?.type === "image" && typeof p.after === "string") {
    return (
      <div className="pv split">
        {typeof p.before === "string" && <img src={p.before} alt="" />}
        <img src={p.after} alt="" style={typeof p.before === "string" ? { clipPath: "inset(0 0 0 50%)", position: "absolute", inset: 0 } : undefined} />
        {typeof p.before === "string" && (
          <>
            <i className="pvline" style={{ left: "50%" }} />
            <span className="pvtag l">{t("library.before")}</span>
            <span className="pvtag r">{t("library.after")}</span>
          </>
        )}
      </div>
    );
  }
  if ((p?.type === "wave" || p?.type === "blocks") && Array.isArray(p.after)) {
    const Draw = p.type === "wave" ? Bars : Blocks;
    const before = Array.isArray(p.before) ? (p.before as number[]) : null;
    return (
      <div className="pv pair" ref={box}>
        {before && (
          <div>
            <Draw values={before} tone="var(--faint)" />
            <span className="pvtag l">{t("library.before")}</span>
          </div>
        )}
        <div>
          <Draw values={p.after as number[]} tone="var(--accent)" />
          {before && <span className="pvtag r">{t("library.after")}</span>}
        </div>
      </div>
    );
  }
  if (item.kind !== "effect") return null;
  // Эффект только на громкость: волна до и после.
  if (targets.includes("audio") && !targets.includes("video")) {
    const fx = [fxOf(item)];
    const after = SPEECH.map((v, i) => v * effectsAt(fx, Math.round((i / SPEECH.length) * LEN), LEN, FPS).volume);
    return (
      <div className="pv pair">
        <div>
          <Bars values={SPEECH} tone="var(--faint)" />
          <span className="pvtag l">{t("library.before")}</span>
        </div>
        <div>
          <Bars values={after} tone="var(--accent)" />
          <span className="pvtag r">{t("library.after")}</span>
        </div>
      </div>
    );
  }
  return <EffectSplit item={item} values={values} live={live} />;
}
