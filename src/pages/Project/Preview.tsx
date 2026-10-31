import { convertFileSrc } from "@tauri-apps/api/core";
import { Pause, Play, Repeat, Volume2, VolumeX } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { MediaItem } from "@/shared/ipc/bindings";

function clock(s: number): string {
  if (!Number.isFinite(s)) return "0:00.0";
  const m = Math.floor(s / 60);
  const sec = s - m * 60;
  return `${m}:${sec.toFixed(1).padStart(4, "0")}`;
}

function size(bytes: number): string {
  return bytes > 1 << 20 ? `${(bytes / (1 << 20)).toFixed(1)} МБ` : `${Math.ceil(bytes / 1024)} КБ`;
}

/** Предпросмотр файла медиа: видео и звук со своими кнопками (пробел — пуск/пауза, стрелки — по
 *  0,1 с), у звука — волна с отметкой и переходом по щелчку; картинка — по размеру. */
export function Preview({ item, projectPath }: { item: MediaItem; projectPath: string }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLVideoElement & HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [dur, setDur] = useState(item.probe?.durationS ?? 0);
  const [muted, setMuted] = useState(false);
  const [loop, setLoop] = useState(true);
  const src = convertFileSrc(`${projectPath}\\${item.path.replaceAll("/", "\\")}`);
  const isVideo = item.category === "footage";
  const isImage = item.category === "images";

  useEffect(() => {
    setPlaying(false);
    setTime(0);
  }, [item.path]);

  const toggle = () => {
    const el = ref.current;
    if (!el) return;
    if (el.paused) void el.play();
    else el.pause();
  };
  const seek = (s: number) => {
    if (ref.current) ref.current.currentTime = Math.max(0, Math.min(dur || 0, s));
  };

  const info = [item.probe?.width && item.probe.height ? `${item.probe.width}×${item.probe.height}` : null, item.probe?.durationS ? clock(item.probe.durationS) : null, size(item.size)].filter(Boolean).join(" · ");

  return (
    <div
      className="preview"
      tabIndex={0}
      onKeyDown={(e) => {
        if (isImage) return;
        if (e.code === "Space") {
          e.preventDefault();
          toggle();
        }
        if (e.key === "ArrowLeft") seek(time - 0.1);
        if (e.key === "ArrowRight") seek(time + 0.1);
      }}
    >
      <div className={`stage ${isImage ? "img" : ""}`}>
        {isImage ? (
          <img src={src} alt="" />
        ) : isVideo ? (
          <video ref={ref} src={src} muted={muted} loop={loop} onClick={toggle} onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)} onLoadedMetadata={(e) => setDur(e.currentTarget.duration)} />
        ) : (
          <div className="wavebox" onClick={(e) => seek(((e.clientX - e.currentTarget.getBoundingClientRect().left) / e.currentTarget.clientWidth) * dur)}>
            {item.thumb ? <img src={convertFileSrc(item.thumb)} alt="" /> : <div className="nowave">{t("preview.noWave")}</div>}
            <div className="played" style={{ width: `${dur ? (time / dur) * 100 : 0}%` }} />
            <audio ref={ref} src={src} muted={muted} loop={loop} onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)} onLoadedMetadata={(e) => setDur(e.currentTarget.duration)} />
          </div>
        )}
      </div>
      {!isImage && (
        <div className="transport">
          <button className="playbtn" onClick={toggle} title={t("preview.play")}>
            {playing ? <Pause size={15} strokeWidth={2} /> : <Play size={15} strokeWidth={2} />}
          </button>
          <span className="tc mono">{clock(time)}</span>
          <input className="scrub" type="range" min={0} max={dur || 0} step={0.01} value={time} onChange={(e) => seek(Number(e.target.value))} style={{ ["--p" as string]: `${dur ? (time / dur) * 100 : 0}%` }} />
          <span className="tc mono faint">{clock(dur)}</span>
          <button className={`iconbtn ${loop ? "on" : ""}`} title={t("preview.loop")} onClick={() => setLoop((l) => !l)}>
            <Repeat size={14} strokeWidth={1.75} />
          </button>
          <button className="iconbtn" title={t("preview.mute")} onClick={() => setMuted((m) => !m)}>
            {muted ? <VolumeX size={14} strokeWidth={1.75} /> : <Volume2 size={14} strokeWidth={1.75} />}
          </button>
        </div>
      )}
      <div className="pinfo">
        <b>{item.name}</b>
        <span className="mono">{info}</span>
      </div>
    </div>
  );
}
