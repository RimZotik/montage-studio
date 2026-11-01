/** Кадр → «ММ:СС:КК» (кадры внутри секунды). */
export function timecode(frame: number, fps: number): string {
  const f = Math.max(0, Math.round(frame));
  const s = Math.floor(f / fps);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}:${String(f % fps).padStart(2, "0")}`;
}

/** Ближайшая точка привязки в пределах `radius` кадров (или само значение). */
export function snap(frame: number, points: number[], radius: number): number {
  let best = frame;
  let dist = radius + 1;
  for (const p of points) {
    const d = Math.abs(p - frame);
    if (d < dist) {
      dist = d;
      best = p;
    }
  }
  return dist <= radius ? best : frame;
}
