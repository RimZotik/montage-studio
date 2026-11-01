import { describe, expect, it } from "vitest";
import { snap, timecode } from "./time";

describe("время таймлайна", () => {
  it("таймкод", () => {
    expect(timecode(0, 60)).toBe("00:00:00");
    expect(timecode(125, 60)).toBe("00:02:05");
    expect(timecode(60 * 61 + 3, 60)).toBe("01:01:03");
  });

  it("привязка к ближайшей точке в радиусе", () => {
    expect(snap(98, [0, 100, 200], 5)).toBe(100);
    expect(snap(90, [0, 100, 200], 5)).toBe(90);
    expect(snap(151, [148, 153], 5)).toBe(153);
  });
});
