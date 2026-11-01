import { describe, expect, it } from "vitest";
import type { Clip } from "@/shared/ipc/bindings";
import { runsOf, visibleRuns, withTrackEffects } from "./TimelineComposition";

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

describe("наложение на одной дорожке", () => {
  const clip = (id: string, track: string, start: number, duration: number, offset = 0) => ({ id, track, kind: "media", src: `media/${id}.mp4`, text: null, start, duration, offset, link: null, props: { speed: 1 } }) as never;
  it("верхний заменяет нижний, нижний продолжается с нужного места файла", () => {
    const r = visibleRuns(runsOf([clip("a", "v1", 0, 100, 10), clip("b", "v1", 30, 20)]));
    const a = r.filter((x) => x.id.startsWith("a"));
    expect(a.map((x) => [x.start, x.duration, x.offset])).toEqual([[0, 30, 10], [50, 50, 60]]);
    expect(r.filter((x) => x.id.startsWith("b")).map((x) => [x.start, x.duration])).toEqual([[30, 20]]);
  });
  it("в один кадр играет только один клип дорожки", () => {
    const r = visibleRuns(runsOf([clip("a", "a1", 0, 50), clip("b", "a1", 20, 50), clip("c", "a1", 10, 15)]));
    for (let f = 0; f < 70; f++) expect(r.filter((x) => f >= x.start && f < x.start + x.duration).length).toBe(1);
  });
  it("другие дорожки не перекрываются", () => {
    expect(visibleRuns(runsOf([clip("a", "v1", 0, 50), clip("b", "a1", 0, 50)])).length).toBe(2);
  });
  it("полностью закрытый клип не проигрывается", () => {
    expect(visibleRuns(runsOf([clip("a", "v1", 10, 10), clip("b", "v1", 0, 50)])).filter((x) => x.id.startsWith("a"))).toEqual([]);
  });
});

describe("эффекты дорожки", () => {
  const fx = (effect: string) => ({ id: "fx1", effect, name: effect, enabled: true, params: {}, controls: [], spec: {} }) as never;
  const c = (id: string, track: string, effects: never[] = []) => ({ id, track, effects }) as never;
  const t = (id: string, effects: never[]) => ({ id, effects }) as never;
  it("действуют на все клипы дорожки и идут раньше своих", () => {
    const out = withTrackEffects([c("a", "s1", [fx("own")]), c("b", "s1"), c("v", "v1")], [t("s1", [fx("look")]), t("v1", [])]) as unknown as { effects: { effect: string }[] }[];
    expect(out[0]?.effects.map((e) => e.effect)).toEqual(["look", "own"]);
    expect(out[1]?.effects.map((e) => e.effect)).toEqual(["look"]);
    expect(out[2]?.effects).toEqual([]);
  });
});
