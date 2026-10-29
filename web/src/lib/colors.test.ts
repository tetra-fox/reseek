import { describe, expect, it } from "vitest";

import { DARK, LIGHT, NAME_HUES, nameColor, nameHue } from "./colors";

/** linear sRGB of an OKLCH color, unclamped */
const linear = (l: number, c: number, h: number) => {
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const L = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const M = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const S = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S,
    -1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S,
    -0.0041960863 * L - 0.7034186147 * M + 1.707614701 * S
  ];
};

/**
 * The relative luminance an OKLCH color shows as: chroma is lowered until the color fits in
 * sRGB, as a browser maps one that doesn't, keeping its lightness
 */
const luminance = (l: number, c: number, h: number) => {
  let low = 0;
  let high = c;
  const fits = (chroma: number) => linear(l, chroma, h).every((v) => v >= -1e-4 && v <= 1 + 1e-4);
  if (!fits(c)) {
    for (let i = 0; i < 30; i++) {
      const mid = (low + high) / 2;
      if (fits(mid)) low = mid;
      else high = mid;
    }
    c = low;
  }
  const [r, g, b] = linear(l, c, h).map((v) => Math.min(1, Math.max(0, v)));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};

const contrast = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

describe("username colors", () => {
  it("give a name the same color every time, from the palette", () => {
    expect(nameHue("alice")).toBe(nameHue("alice"));
    expect(nameColor("bob")).toBe(nameColor("bob"));
    expect(NAME_HUES).toContain(nameHue("ж€😀"));
    expect(nameColor("alice")).toMatch(/^light-dark\(oklch\(.+\), oklch\(.+\)\)$/);
  });

  it("spread names evenly over the palette", () => {
    const counts = new Map<number, number>();
    const total = 12_000;
    for (let i = 0; i < total; i++) {
      const hue = nameHue(`user${i}`);
      counts.set(hue, (counts.get(hue) ?? 0) + 1);
    }
    const mean = total / NAME_HUES.length;
    expect(counts.size).toBe(NAME_HUES.length);
    for (const n of counts.values()) expect(Math.abs(n - mean) / mean).toBeLessThan(0.15);
    // names one letter apart rarely share a color
    const pairs = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];
    const same = pairs.filter((n) => nameHue(n) === nameHue(`${n}2`)).length;
    expect(same).toBeLessThan(3);
  });

  it("read as text on both themes' surfaces", () => {
    // the surfaces names sit on: plain, hovered and selected rows, light then dark
    const light = [luminance(1, 0, 0), luminance(0.95, 0.006, 255), luminance(0.955, 0.03, 252)];
    const dark = [
      luminance(0.168, 0.007, 255),
      luminance(0.225, 0.009, 255),
      luminance(0.27, 0.065, 252)
    ];
    for (const h of NAME_HUES) {
      const onLight = luminance(LIGHT.l, LIGHT.c, h);
      const onDark = luminance(DARK.l, DARK.c, h);
      for (const surface of light) expect(contrast(onLight, surface)).toBeGreaterThanOrEqual(4.5);
      for (const surface of dark) expect(contrast(onDark, surface)).toBeGreaterThanOrEqual(4.5);
    }
  });
});
