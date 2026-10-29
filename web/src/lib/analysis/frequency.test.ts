import { describe, expect, it } from "vitest";

import { axisTicks, binHz, heightOf, heightOn, hzAt, hzOn, mix, pure, rowBins } from "./frequency";

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
    expect(rowBins(pure("linear"), 22_050, 512, 1)[0]).toBeCloseTo(255.5, 6);
    // the bottom of a log axis is 20 Hz, under the middle of bin 0, so bin 0 alone
    expect(rowBins(pure("log"), 22_050, 512, 4096)[4095]).toBe(0);
    // a row's frequency over every bin's middle reads the top bin
    expect(rowBins(pure("mel"), 22_050, 512, 4096)[0]).toBe(511);
  });

  it("places each row's bins near where its own frequency falls", () => {
    // between two bin middles a row is placed by height, not by hertz, so on a log axis it
    // sits up to a fraction of a bin off where its frequency would put it
    for (const scale of ["linear", "log", "mel"] as const) {
      const rows = rowBins(pure(scale), 22_050, 512, 700);
      rows.forEach((at, row) => {
        const hz = hzAt(scale, 22_050, (row + 0.5) / 700);
        const exact = Math.min(511, Math.max(0, (hz / 22_050) * 512 - 0.5));
        expect(Math.abs(at - exact)).toBeLessThan(0.15);
      });
    }
  });

  it("reads the bins in order down the rows of an axis between scales", () => {
    const rows = rowBins(mix(pure("log"), pure("mel"), 0.4), 22_050, 512, 700);
    for (let row = 1; row < rows.length; row++)
      expect(rows[row]).toBeLessThanOrEqual(rows[row - 1]);
  });
});

describe("an axis between two scales", () => {
  const PAIRS = [
    ["linear", "log"],
    ["log", "mel"],
    ["mel", "linear"]
  ] as const;
  const FREQUENCIES = [0, 5, 20, 21, 100, 440, 1_000, 4_000, 11_025, 16_000, 20_000, 22_050];

  it("is each scale's own axis at its ends", () => {
    for (const [from, to] of PAIRS) {
      for (const hz of FREQUENCIES) {
        expect(heightOn(mix(pure(from), pure(to), 0), 22_050, hz)).toBe(heightOf(from, 22_050, hz));
        expect(heightOn(mix(pure(from), pure(to), 1), 22_050, hz)).toBe(heightOf(to, 22_050, hz));
      }
    }
  });

  it("keeps higher frequencies higher all the way between", () => {
    for (const [from, to] of PAIRS) {
      for (let step = 0; step <= 20; step++) {
        const curve = mix(pure(from), pure(to), step / 20);
        let last = Number.POSITIVE_INFINITY;
        for (let hz = 0; hz <= 22_050; hz += 7) {
          const height = heightOn(curve, 22_050, hz);
          expect(height).toBeLessThanOrEqual(last);
          last = height;
        }
        expect(heightOn(curve, 22_050, 0)).toBe(1);
        expect(heightOn(curve, 22_050, 22_050)).toBe(0);
      }
    }
  });

  it("finds the frequency at a height", () => {
    const curve = mix(pure("linear"), pure("log"), 0.3);
    for (const hz of [10, 440, 16_000]) {
      expect(hzOn(curve, 22_050, heightOn(curve, 22_050, hz))).toBeCloseTo(hz, 3);
    }
  });

  it("fades the labels that come too close and those of the scale it is leaving", () => {
    // on a 100 px linear axis 5 kHz labels are 23 px apart, so with 40 px of room one in two shows
    const linear = axisTicks(pure("linear"), 22_050, 100, 40);
    expect(linear.map((t) => [t.hz, t.alpha])).toEqual([
      [0, 1],
      [10_000, 1],
      [20_000, 1]
    ]);
    // a quarter of the way to log, the labels both scales have stay, linear's own are half gone
    // and log's own have not come in yet
    const between = axisTicks(mix(pure("linear"), pure("log"), 0.25), 22_050, 1000, 1);
    expect(between.find((t) => t.hz === 5_000)?.alpha).toBe(1);
    expect(between.find((t) => t.hz === 15_000)?.alpha).toBe(0.5);
    expect(between.find((t) => t.hz === 100)).toBeUndefined();
  });

  it("shows each label whole or not at all on a scale", () => {
    for (const scale of ["linear", "log", "mel"] as const) {
      for (let length = 40; length <= 800; length += 7) {
        for (const tick of axisTicks(pure(scale), 22_050, length, 20)) expect(tick.alpha).toBe(1);
      }
    }
  });

  it("fades labels in and out without a jump all the way between two scales", () => {
    for (const [from, to] of [
      ["linear", "log"],
      ["log", "mel"],
      ["mel", "linear"]
    ] as const) {
      const alphas = (t: number) =>
        new Map(
          axisTicks(mix(pure(from), pure(to), t), 22_050, 300, 40).map((k) => [k.hz, k.alpha])
        );
      let last = alphas(0);
      for (let step = 1; step <= 1000; step++) {
        const now = alphas(step / 1000);
        for (const hz of new Set([...last.keys(), ...now.keys()])) {
          expect(Math.abs((now.get(hz) ?? 0) - (last.get(hz) ?? 0))).toBeLessThan(0.1);
        }
        last = now;
      }
    }
  });
});
