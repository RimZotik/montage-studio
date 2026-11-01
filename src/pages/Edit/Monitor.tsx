import { Player, type PlayerRef } from "@remotion/player";
import { Pause, Play, Scan, ShieldAlert, SkipBack, Volume2, VolumeX } from "lucide-react";
import { type RefObject, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { type ProjectView, type SmartState, commands as ipc } from "@/shared/ipc/bindings";
import { TimelineComposition } from "@/render/TimelineComposition";
import { useSetting } from "@/shared/settings/settings";
import { timecode } from "./time";

/** Монитор: плеер композиции «таймлайн» (та же, что пойдет в рендер), транспорт, безопасная зона. */
export function Monitor({ view, total, playerRef, srcOf, frame, playing, smart }: { smart: SmartState | null; view: ProjectView; total: number; playerRef: RefObject<PlayerRef | null>; srcOf: (p: string) => string; frame: number; playing: boolean }) {
  const { t } = useTranslation();
  const seq = view.project.sequence;
  const [safe, setSafe] = useSetting("monitor.safeZone", true);
  const [volume, setVolume] = useSetting("monitor.volume", 1);
  const [muted, setMuted] = useSetting("monitor.muted", false);
  useEffect(() => {
    const p = playerRef.current;
    if (!p) return;
    p.setVolume(volume);
    if (muted) p.mute();
    else p.unmute();
  }, [volume, muted, playerRef]);
  const inputProps = useMemo(() => ({ sequence: seq, tracks: view.tracks, clips: view.clips, srcOf, smart: smart?.sources }), [seq, view.tracks, view.clips, srcOf, smart?.sources]);
  const vertical = seq.height > seq.width;

  useEffect(() => {
    // Плеер не знает о смене длительности до следующего кадра — вернуть головку в пределы.
    if (frame >= total) playerRef.current?.seekTo(Math.max(0, total - 1));
  }, [total, frame, playerRef]);

  return (
    <div className="monitor">
      <div className="screen">
        <div className="frame" style={{ aspectRatio: `${seq.width} / ${seq.height}` }}>
          <Player
            ref={playerRef}
            component={TimelineComposition}
            inputProps={inputProps}
            durationInFrames={Math.max(1, total)}
            compositionWidth={seq.width}
            compositionHeight={seq.height}
            fps={seq.fps}
            style={{ width: "100%", height: "100%" }}
            clickToPlay={false}
            doubleClickToFullscreen
            spaceKeyToPlayOrPause={false}
            acknowledgeRemotionLicense
          />
          {safe && vertical && <div className="safezone" />}
          {smart?.hasCode && !smart.trusted && (
            <div className="smartask">
              <ShieldAlert size={20} strokeWidth={1.75} />
              <p>{t("smart.ask")}</p>
              <button className="btn pri" onClick={() => void ipc.smartTrust()}>
                {t("smart.trust")}
              </button>
            </div>
          )}
        </div>
      </div>
      <div className="transport">
        <button className="iconbtn" title="Home" onClick={() => playerRef.current?.seekTo(0)}>
          <SkipBack size={14} strokeWidth={1.75} />
        </button>
        <button className="playbtn" title={t("preview.play")} onClick={() => playerRef.current?.toggle()}>
          {playing ? <Pause size={15} strokeWidth={2} /> : <Play size={15} strokeWidth={2} />}
        </button>
        <span className="tc mono">{timecode(frame, seq.fps)}</span>
        <span className="tc mono faint">/ {timecode(total, seq.fps)}</span>
        <span style={{ flex: 1 }} />
        <button className="iconbtn" title={t("preview.mute")} onClick={() => setMuted((m) => !m)}>
          {muted || volume === 0 ? <VolumeX size={14} strokeWidth={1.75} /> : <Volume2 size={14} strokeWidth={1.75} />}
        </button>
        <input
          className="scrub vol"
          type="range"
          min={0}
          max={1}
          step={0.01}
          value={muted ? 0 : volume}
          title={`${Math.round(volume * 100)}%`}
          onChange={(e) => {
            setMuted(false);
            setVolume(Number(e.target.value));
          }}
          style={{ ["--p" as string]: `${(muted ? 0 : volume) * 100}%` }}
        />
        {vertical && (
          <button className={`btn quiet safebtn ${safe ? "on" : ""}`} title={t("edit.safeHint")} onClick={() => setSafe((s) => !s)}>
            <Scan size={14} strokeWidth={1.75} />
            {t("edit.safe")}
          </button>
        )}
      </div>
    </div>
  );
}
