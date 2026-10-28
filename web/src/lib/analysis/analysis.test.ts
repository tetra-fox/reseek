import { describe, expect, it } from "vitest";

import { heightOf, type Layout, windowAt } from "./analysis";

const layout = (scale: "log" | "linear", plotWidth = 40): Layout => ({
  width: 48 + plotWidth + 222,
  height: 556,
  plotX: 48,
  plotY: 14,
  plotWidth,
  plotHeight: 512,
  scale,
  minHz: scale === "log" ? 20 : 0,
  maxHz: 22_050,
  sampled: false,
  guides: []
});

describe("heightOf", () => {
  // the rows the daemon draws these lines on, worked out by hand for a 512-row plot
  it("puts a frequency on the row the daemon draws it on", () => {
    for (const [scale, hz, row] of [
      ["linear", 16_000, 140],
      ["linear", 20_000, 47],
      ["log", 16_000, 23],
      ["log", 19_500, 8]
    ] as const) {
      expect(Math.floor(heightOf(layout(scale), hz) * 512)).toBe(row);
    }
  });

  it("keeps a frequency off the plot at its edge", () => {
    expect(heightOf(layout("log"), 5)).toBe(1);
    expect(heightOf(layout("linear"), 30_000)).toBe(0);
  });
});

describe("windowAt", () => {
  it("finds a window by its column, and by its run of windows past the column count", () => {
    expect(windowAt(layout("log", 40), 0, 40)).toBe(0);
    expect(windowAt(layout("log", 40), 0.999, 40)).toBe(39);
    expect(windowAt(layout("log", 40), 0.5, 40)).toBe(20);
    // 4000 windows shown as 2000 columns, two to a column
    expect(windowAt(layout("log", 2000), 0.5, 4000)).toBe(2000);
    expect(windowAt(layout("log", 2000), 1, 4000)).toBe(3998);
  });
});
