import { convertFileSrc } from "@tauri-apps/api/core";
import { Player } from "@remotion/player";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Captions, CircleX, Clapperboard, Download, Folder, FolderOpen, Loader, Play, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { TimelineComposition } from "@/render/TimelineComposition";
import { type AppError, type ExportProgress, type ExportStatus, type ProjectView, commands as ipc, events } from "@/shared/ipc/bindings";

const QUALITY = ["high", "max", "compact"] as const;
const CODEC = ["h264", "h265"] as const;
/** Примерный поток кадра 1080×1920 при каждом качестве, Мбит/с (для подсказки размера). */
const MBPS: Record<string, number> = { max: 24, high: 12, compact: 5 };

/** «Экспорт»: качество словами вместо сотни параметров, ход рендера, готовые файлы. */
export function ExportPage({ view, onError }: { view: ProjectView; onError: (e: AppError) => void }) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ExportStatus | null>(null);
  const [progress, setProgress] = useState<ExportProgress | null>(null);
  const [quality, setQuality] = useState<(typeof QUALITY)[number]>("high");
  const [codec, setCodec] = useState<(typeof CODEC)[number]>("h264");
  const [half, setHalf] = useState(false);
  const [name, setName] = useState("");
  const [playing, setPlaying] = useState<string | null>(null);
  const [sure, setSure] = useState<string | null>(null);

  const reload = useCallback(() => void ipc.exportStatus().then((r) => (r.status === "ok" ? setStatus(r.data) : onError(r.error))), [onError]);
  useEffect(() => {
    reload();
    const off = events.exportProgress.listen(({ payload }) => {
      setProgress(payload);
      if (["done", "error", "cancelled", "installed"].includes(payload.phase)) reload();
    });
    return () => void off.then((f) => f());
  }, [reload]);

  const seq = view.project.sequence;
  const frames = useMemo(() => Math.max(0, ...view.clips.map((c) => c.start + c.duration)), [view.clips]);
  const seconds = frames / seq.fps;
  const estimate = (seconds * (MBPS[quality] ?? 12) * (half ? 0.3 : 1)) / 8;
  const phase = progress?.phase;
  const installing = phase === "install";
  const running = !!status?.running || phase === "render" || phase === "encode" || phase === "finish";
  const pct = Math.round((progress?.progress ?? 0) * 100);

  const srcOf = useCallback((p: string) => convertFileSrc(`${view.path}\\${p.replaceAll("/", "\\")}`), [view.path]);
  const preview = useMemo(() => ({ sequence: seq, tracks: view.tracks, clips: view.clips, srcOf }), [seq, view.tracks, view.clips, srcOf]);
  const soft = view.tracks.some((t) => t.kind === "captions" && t.captionMode === "file");

  const chooseDir = async () => {
    const dir = await openDialog({ directory: true, defaultPath: status?.dir ?? undefined });
    if (typeof dir !== "string") return;
    const r = await ipc.exportSetDir(dir);
    if (r.status === "error") return onError(r.error);
    reload();
  };

  const removeFile = async (path: string) => {
    if (sure !== path) return setSure(path);
    setSure(null);
    if (playing === path) setPlaying(null);
    const r = await ipc.exportDelete(path);
    if (r.status === "error") return onError(r.error);
    reload();
  };

  const start = async () => {
    setProgress({ phase: "render", progress: 0, message: null, output: null });
    const r = await ipc.exportStart({ name, codec, quality, scale: half ? 0.5 : 1 });
    if (r.status === "error") {
      setProgress(null);
      onError(r.error);
    } else reload();
  };

  return (
    <div className="exportpage">
      <section className="excol">
        <h2>{t("export.title")}</h2>
        <p className="faint exsum">
          {seq.width * (half ? 0.5 : 1)}×{seq.height * (half ? 0.5 : 1)} · {seq.fps} fps · {seconds.toFixed(1)} {t("chat.sec")}
        </p>
        <label className="ctl">
          <span className="k">{t("export.name")}</span>
          <input className="field" placeholder={view.project.name} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <div className="ctlsec">{t("export.quality")}</div>
        {QUALITY.map((q) => (
          <button key={q} className={`exopt ${quality === q ? "on" : ""}`} onClick={() => setQuality(q)}>
            <b>{t(`export.q.${q}.name`)}</b>
            <span className="faint">{t(`export.q.${q}.hint`)}</span>
          </button>
        ))}
        <div className="ctlsec">{t("export.more")}</div>
        <label className="ctl">
          <span className="k">{t("export.codec")}</span>
          <select className="field" value={codec} onChange={(e) => setCodec(e.target.value as (typeof CODEC)[number])}>
            {CODEC.map((c) => (
              <option key={c} value={c}>
                {t(`export.c.${c}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="ctl check">
          <input type="checkbox" checked={half} onChange={(e) => setHalf(e.target.checked)} />
          <span>{t("export.half")}</span>
        </label>
        <div className="ctlsec">{t("export.folder")}</div>
        <div className="exdir">
          <Folder size={14} strokeWidth={1.75} />
          <span title={status?.dir ?? ""}>{status?.dir ?? ""}</span>
          <button className="iconbtn" title={t("export.folderChoose")} onClick={() => void chooseDir()}>
            <FolderOpen size={14} strokeWidth={1.75} />
          </button>
          <button className="iconbtn" title={t("export.folderReset")} onClick={() => void ipc.exportSetDir(null).then(reload)}>
            <RotateCcw size={13} strokeWidth={1.75} />
          </button>
        </div>
        {soft && (
          <p className="faint exsum ic">
            <Captions size={14} strokeWidth={1.75} />
            {t("export.softCaptions")}
          </p>
        )}
        <p className="faint exsum">{t("export.estimate", { mb: Math.max(1, Math.round(estimate)) })}</p>

        {status && !status.engineReady ? (
          <div className="exengine">
            <p className="faint">{status.nodeFound ? t("export.engineNeeded") : t("export.nodeNeeded")}</p>
            <button className="btn pri" disabled={installing || !status.nodeFound} onClick={() => { setProgress({ phase: "install", progress: 0, message: null, output: null }); void ipc.exportEngineInstall(); }}>
              {installing ? <Loader size={14} strokeWidth={1.75} className="spin" /> : <Download size={14} strokeWidth={1.75} />}
              {installing ? `${pct} %` : t("export.engineInstall")}
            </button>
          </div>
        ) : running ? (
          <div className="exrun">
            <div className="exphase">
              <span className="ic">
                <Loader size={14} strokeWidth={1.75} className="spin" />
                {t(`export.phase.${phase ?? "render"}`, { defaultValue: t("export.phase.render") })}
              </span>
              <span className="mono">{pct} %</span>
            </div>
            <div className="bar exbar">
              <i style={{ width: `${pct}%` }} />
            </div>
            <button className="btn" onClick={() => void ipc.exportCancel()}>
              <CircleX size={14} strokeWidth={1.75} />
              {t("export.cancel")}
            </button>
          </div>
        ) : (
          <button className="btn pri exgo" disabled={!frames || !status} onClick={() => void start()}>
            <Clapperboard size={14} strokeWidth={1.75} />
            {t("export.start")}
          </button>
        )}
        {phase === "error" && progress?.message && <p className="exerr">{progress.message}</p>}
      </section>

      <section className="exfiles">
        <div className="exstage">
          {playing ? (
            <video key={playing} className="explay" src={convertFileSrc(playing)} controls autoPlay />
          ) : frames > 0 ? (
            <div className="exframe" style={{ aspectRatio: `${seq.width} / ${seq.height}` }}>
              <Player component={TimelineComposition} inputProps={preview} durationInFrames={Math.max(1, frames)} compositionWidth={seq.width} compositionHeight={seq.height} fps={seq.fps} controls style={{ width: "100%", height: "100%" }} acknowledgeRemotionLicense />
            </div>
          ) : (
            <p className="faint">{t("export.noFrames")}</p>
          )}
          <p className="faint exstagecap">{playing ? status?.files.find((f) => f.path === playing)?.name : t("export.previewCap")}</p>
        </div>
        <div className="exlist">
          <div className="exlisthead">
            <h2>{t("export.files")}</h2>
            {playing && (
              <button className="btn quiet" onClick={() => setPlaying(null)}>
                {t("export.backToProject")}
              </button>
            )}
          </div>
          {status?.files.length ? (
            status.files.map((f) => (
              <div key={f.path} className={`exfile ${playing === f.path ? "on" : ""}`} onDoubleClick={() => setPlaying(f.path)}>
                <button className="iconbtn" title={t("export.play")} onClick={() => setPlaying(f.path)}>
                  <Play size={14} strokeWidth={1.75} />
                </button>
                <span className="exname" title={f.path}>
                  {f.name}
                </span>
                {!!f.captions.length && (
                  <span className="exbadge" title={t("export.captionFiles", { list: f.captions.join(", ") })}>
                    <Captions size={13} strokeWidth={1.75} />
                  </span>
                )}
                <span className="faint mono">
                  {f.width}×{f.height} · {f.durationS?.toFixed(1)} {t("chat.sec")} · {(f.sizeMb ?? 0).toFixed(1)} {t("export.mb")}
                  {f.mbps ? ` · ${f.mbps.toFixed(1)} ${t("export.mbps")}` : ""}
                </span>
                <button className="iconbtn" title={t("export.reveal")} onClick={() => void revealItemInDir(f.path)}>
                  <FolderOpen size={14} strokeWidth={1.75} />
                </button>
                <button className={`iconbtn ${sure === f.path ? "danger" : ""}`} title={sure === f.path ? t("export.deleteSure") : t("export.delete")} onBlur={() => setSure(null)} onClick={() => void removeFile(f.path)}>
                  <Trash2 size={14} strokeWidth={1.75} />
                </button>
              </div>
            ))
          ) : (
            <p className="faint">{t("export.noFiles")}</p>
          )}
        </div>
      </section>
    </div>
  );
}
