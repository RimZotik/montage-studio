import { describe, expect, it } from "vitest";
import type { Clip } from "@/shared/ipc/bindings";
import { runsOf } from "./TimelineComposition";

const clip = (id: string, start: number, duration: number, offset: number, src = "media/footage/a.mp4", volume = 1): Clip => ({ id, track: "v1", kind: "media", src, text: null, start, duration, offset, link: null, props: { volume, opacity: 1, scale: 1, x: 0, y: 0, fadeIn: 0, fadeOut: 0 } });

describe("непрерывные куски одного файла", () => {
  it("части встык играются одним куском, даже с разной громкостью", () => {
    const runs = runsOf([clip("c1", 0, 60, 0), clip("c2", 60, 40, 60, undefined, 0.3)]);
    expect(runs).toHaveLength(1);
    expect(runs[0]).toMatchObject({ start: 0, duration: 100, offset: 0 });
    expect(runs[0]?.parts.map((p) => p.id)).toEqual(["c1", "c2"]);
  });

  it("разрыв во времени, другой файл или другой кусок файла — отдельно", () => {
    expect(runsOf([clip("c1", 0, 60, 0), clip("c2", 61, 40, 61)])).toHaveLength(2);
    expect(runsOf([clip("c1", 0, 60, 0), clip("c2", 60, 40, 60, "media/footage/b.mp4")])).toHaveLength(2);
    expect(runsOf([clip("c1", 0, 60, 0), clip("c2", 60, 40, 200)])).toHaveLength(2);
  });

  it("порядок в списке — выше тот, что позже", () => {
    const runs = runsOf([clip("c1", 0, 60, 0, "media/footage/b.mp4"), clip("c2", 30, 60, 0)]);
    expect(runs.find((r) => r.id === "c2")?.z).toBeGreaterThan(runs.find((r) => r.id === "c1")?.z ?? 99);
  });
});
