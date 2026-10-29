import { describe, expect, it } from "vitest";

import { binHz, binsAt, heightOf, hzAt } from "./frequency";

describe("heightOf", () => {
  // the rows these lines fall on in a 512-row plot of a 44.1 kHz file, worked out by hand
  it("puts a frequency on its row on each scale", () => {
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
      expect(Math.floor(heightOf(scale, 22_050, hz) * 512)).toBe(row);
    }
  });

  it("keeps a frequency off the axis at its edge", () => {
    expect(heightOf("log", 22_050, 5)).toBe(1);
    expect(heightOf("linear", 22_050, 30_000)).toBe(0);
  });

  it("is undone by hzAt on every scale", () => {
    for (const scale of ["linear", "log", "mel"] as const) {
      for (const hz of [40, 1_000, 16_000, 21_000]) {
        expect(hzAt(scale, 22_050, heightOf(scale, 22_050, hz))).toBeCloseTo(hz, 6);
      }
    }
  });
});

describe("the daemon's bins on each scale", () => {
  // the daemon keeps 512 linear bins from 0 Hz to half the rate; bin 100 of a 44.1 kHz file is
  // centered on 4328.17 Hz, bin 371 holds 16 kHz
  it("puts each bin where its frequency falls", () => {
    expect(binHz(100, 512, 22_050)).toBeCloseTo(4328.17, 2);
    expect(binHz(511, 512, 22_050)).toBeCloseTo(22_028.5, 1);
    for (const [scale, bin, row] of [
      ["linear", 371, 140],
      ["log", 371, 23],
      ["mel", 371, 45]
    ] as const) {
      expect(Math.floor(heightOf(scale, 22_050, binHz(bin, 512, 22_050)) * 512)).toBe(row);
    }
  });

  it("reads each row between the two bins around its frequency", () => {
    // the middle of a linear axis is 11025 Hz, half way from the middle of bin 255 to bin 256's
    expect(binsAt("linear", 22_050, 512, 0.5)).toEqual({ below: 255, above: 256, share: 0.5 });
    // the bottom of a log axis is 20 Hz, under the middle of bin 0, so bin 0 alone
    expect(binsAt("log", 22_050, 512, 1)).toEqual({ below: 0, above: 1, share: 0 });
    // a row's frequency over every bin's middle reads the top bin
    expect(binsAt("mel", 22_050, 512, 0)).toEqual({ below: 511, above: 511, share: 0 });
  });
});
