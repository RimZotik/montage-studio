import { describe, expect, it } from "vitest";
import type { Clip } from "@/shared/ipc/bindings";
import { joinRuns } from "./TimelineComposition";

const clip = (id: string, start: number, duration: number, offset: number, src = "media/footage/a.mp4"): Clip => ({ id, track: "v1", kind: "media", src, text: null, start, duration, offset, props: { volume: 1, opacity: 1, scale: 1, x: 0, y: 0, fadeIn: 0, fadeOut: 0 } });

describe("склейка частей после разреза", () => {
  it("части одного файла встык играются одним клипом", () => {
    const out = joinRuns([clip("c1", 0, 60, 0), clip("c2", 60, 40, 60)]);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ start: 0, duration: 100, offset: 0 });
  });

  it("разрыв во времени, другой файл или другой кусок файла — отдельно", () => {
    expect(joinRuns([clip("c1", 0, 60, 0), clip("c2", 61, 40, 61)])).toHaveLength(2);
    expect(joinRuns([clip("c1", 0, 60, 0), clip("c2", 60, 40, 60, "media/footage/b.mp4")])).toHaveLength(2);
    expect(joinRuns([clip("c1", 0, 60, 0), clip("c2", 60, 40, 200)])).toHaveLength(2);
  });
});
