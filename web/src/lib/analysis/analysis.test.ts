import { describe, expect, it } from "vitest";

import { cutoffRows, percentOf } from "./analysis";

describe("cutoffRows", () => {
  it("folds a run of rare cutoffs into one line and keeps common ones apart", () => {
    // 240 windows: 2.4 makes 1%, so the single windows are rare and the 200 and the 3 are not
    const histogram: [number, number][] = [
      [2_000, 1],
      [2_500, 1],
      [3_000, 1],
      [16_000, 200],
      [16_500, 1],
      [20_000, 3],
      [22_000, 1],
      [22_500, 32]
    ];
    expect(cutoffRows(histogram)).toEqual([
      { from: 2_000, to: 3_000, count: 3 },
      { from: 16_000, to: 16_000, count: 200 },
      { from: 16_500, to: 16_500, count: 1 },
      { from: 20_000, to: 20_000, count: 3 },
      { from: 22_000, to: 22_000, count: 1 },
      { from: 22_500, to: 22_500, count: 32 }
    ]);
  });

  it("keeps a histogram with no rare cutoff as it is", () => {
    expect(
      cutoffRows([
        [16_000, 79],
        [24_000, 160]
      ])
    ).toEqual([
      { from: 16_000, to: 16_000, count: 79 },
      { from: 24_000, to: 24_000, count: 160 }
    ]);
  });
});

describe("percentOf", () => {
  it("never rounds a share that is there to 0% or one that is not all to 100%", () => {
    expect(percentOf(1, 240)).toBe("<1%");
    expect(percentOf(239, 240)).toBe(">99%");
    expect(percentOf(0, 240)).toBe("0%");
    expect(percentOf(240, 240)).toBe("100%");
    expect(percentOf(160, 240)).toBe("67%");
  });
});
