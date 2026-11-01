// Задание рендера от ядра студии: `node run.mjs job.json`. Строки вывода — JSON (ход, ошибка).
// Медиа проекта отдает маленький локальный сервер: браузеру движка нельзя читать файлы с диска.

import { createReadStream, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { resolve, sep } from "node:path";
import { rmSync } from "node:fs";
import { ensureBrowser, renderMedia, renderStill, selectComposition } from "@remotion/renderer";

const say = (o) => console.log(JSON.stringify(o));
const jobFile = process.argv[2];
const job = JSON.parse(readFileSync(jobFile, "utf8"));
rmSync(jobFile, { force: true });

const TYPES = { mp4: "video/mp4", mov: "video/quicktime", webm: "video/webm", mkv: "video/x-matroska", mp3: "audio/mpeg", wav: "audio/wav", m4a: "audio/mp4", aac: "audio/aac", ogg: "audio/ogg", opus: "audio/ogg", flac: "audio/flac", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", webp: "image/webp", gif: "image/gif", svg: "image/svg+xml", bmp: "image/bmp" };

/** Только `media/` проекта, с частичной отдачей: видео читается кусками. */
function serveProject(root) {
  const media = resolve(root, "media") + sep;
  const server = createServer((req, res) => {
    res.setHeader("Access-Control-Allow-Origin", "*");
    let path;
    try {
      path = resolve(root, decodeURIComponent(new URL(req.url, "http://x").pathname.slice(1)));
    } catch {
      res.writeHead(400).end();
      return;
    }
    if (!path.startsWith(media)) return void res.writeHead(403).end();
    let size;
    try {
      size = statSync(path).size;
    } catch {
      return void res.writeHead(404).end();
    }
    const type = TYPES[path.split(".").pop()?.toLowerCase() ?? ""] ?? "application/octet-stream";
    const range = /bytes=(\d*)-(\d*)/.exec(req.headers.range ?? "");
    if (range) {
      const start = range[1] ? Number(range[1]) : 0;
      const end = range[2] ? Math.min(Number(range[2]), size - 1) : size - 1;
      res.writeHead(206, { "Content-Type": type, "Accept-Ranges": "bytes", "Content-Range": `bytes ${start}-${end}/${size}`, "Content-Length": end - start + 1 });
      createReadStream(path, { start, end }).pipe(res);
    } else {
      res.writeHead(200, { "Content-Type": type, "Accept-Ranges": "bytes", "Content-Length": size });
      createReadStream(path).pipe(res);
    }
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

try {
  await ensureBrowser({
    onBrowserDownload: () => ({
      version: null,
      onProgress: (p) => say({ type: "browser", progress: p.percent }),
    }),
  });
  if (job.mode === "prepare") {
    say({ type: "done" });
    process.exit(0);
  }
  const server = await serveProject(job.projectDir);
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const inputProps = { ...job.props, baseUrl };
  const composition = await selectComposition({ serveUrl: job.bundle, id: "timeline", inputProps, logLevel: "error" });
  if (job.mode === "still") {
    await renderStill({ composition, serveUrl: job.bundle, inputProps, frame: job.frame, output: job.output, imageFormat: "jpeg", jpegQuality: 70, scale: job.scale ?? 0.5, logLevel: "error" });
  } else {
    await renderMedia({
      composition,
      serveUrl: job.bundle,
      inputProps,
      codec: job.codec,
      crf: job.crf,
      audioCodec: "aac",
      audioBitrate: job.audioBitrate,
      x264Preset: job.codec === "h264" ? job.preset : undefined,
      pixelFormat: "yuv420p",
      colorSpace: "bt709",
      enforceAudioTrack: true,
      scale: job.scale ?? 1,
      outputLocation: job.output,
      logLevel: "error",
      onProgress: (p) => say({ type: "progress", stage: p.stitchStage === "muxing" ? "finish" : p.renderedFrames < composition.durationInFrames ? "render" : "encode", progress: p.progress }),
    });
  }
  server.close();
  say({ type: "done" });
  process.exit(0);
} catch (e) {
  say({ type: "error", message: String(e?.message ?? e).split("\n")[0] });
  console.error(e?.stack ?? e);
  process.exit(1);
}
