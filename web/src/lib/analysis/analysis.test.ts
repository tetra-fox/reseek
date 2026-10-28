import { describe, expect, it } from "vitest";

import { acrossAt, heightOf, type Layout, type Scale, windowAt } from "./analysis";

const layout = (scale: Scale, durationSeconds = 40, plotWidth = 1024): Layout => ({
  width: 48 + plotWidth + 222,
  height: 556,
  plotX: 48,
  plotY: 14,
  plotWidth,
  plotHeight: 512,
  scale,
  minHz: scale === "log" ? 20 : 0,
  maxHz: 22_050,
  durationSeconds,
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
      ["log", 19_500, 8],
      ["mel", 16_000, 45],
      ["mel", 19_000, 21],
      ["mel", 19_500, 17],
      ["mel", 20_000, 13]
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
  it("finds the window at a time across the file's whole length", () => {
    const every = Array.from({ length: 40 }, (_, t) => t);
    expect(windowAt(layout("log", 40), 0, every)).toBe(0);
    expect(windowAt(layout("log", 40), 0.999, every)).toBe(39);
    expect(windowAt(layout("log", 40), 0.5, every)).toBe(20);
  });

  it("takes the nearest sample where a file was read in samples", () => {
    // four one-second samples of a 400-second file
    const samples = [0, 100, 200, 300];
    expect(windowAt(layout("log", 400), 0.26, samples)).toBe(1);
    expect(windowAt(layout("log", 400), 0.9, samples)).toBe(3);
    expect(acrossAt(layout("log", 400), 200)).toBe(0.5);
  });
});
