import { readFileSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { DARK, LIGHT, NAME_HUES, nameHue } from "./colors";

// read from disk, as vitest hands a test every stylesheet import empty, `?raw` too
const css = readFileSync(join(import.meta.dirname, "../../app.css"), "utf8");

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

type Oklch = readonly [l: number, c: number, h: number];

/**
 * The linear sRGB an OKLCH color shows as: chroma is lowered until the color fits in sRGB, as a
 * browser maps one that doesn't, keeping its lightness
 */
const shown = ([l, c, h]: Oklch) => {
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
  return linear(l, c, h).map((v) => Math.min(1, Math.max(0, v)));
};

const encode = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);
const decode = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

/** linear sRGB of `top` at `alpha` over `under`, blended in gamma-encoded sRGB as browsers do */
const over = (top: number[], alpha: number, under: number[]) =>
  top.map((v, i) => decode(alpha * encode(v) + (1 - alpha) * encode(under[i])));

const luminance = ([r, g, b]: number[]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

const contrast = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

/** the declarations of the first rule in app.css that starts with `head` */
const rule = (head: string) => {
  const start = css.indexOf(`\n${head} {`);
  if (start < 0) throw new Error(`app.css has no ${head} rule`);
  let depth = 0;
  for (let i = css.indexOf("{", start); i < css.length; i++) {
    if (css[i] === "{") depth++;
    else if (css[i] === "}" && --depth === 0) return css.slice(start, i);
  }
  throw new Error(`${head} is not closed`);
};

const property = (block: string, name: string) => {
  const value = new RegExp(`--${name}:\\s*([^;]+);`).exec(block)?.[1];
  if (value === undefined) throw new Error(`--${name} is not set`);
  return value.trim();
};

const root = rule(":root");

/** a theme token's `oklch(l c h)`, its hue resolved from the `:root` custom property it names */
const token = (block: string, name: string): Oklch => {
  const value = property(block, `color-${name}`).replace(/var\(--([\w-]+)\)/, (_, v) =>
    property(root, v)
  );
  const parts = /^oklch\(([\d.]+) ([\d.]+) ([\d.]+)\)$/.exec(value);
  if (!parts) throw new Error(`--color-${name} is not a plain oklch(): ${value}`);
  return [Number(parts[1]), Number(parts[2]), Number(parts[3])];
};

/**
 * How opaque a selected row's accent-soft is: mixing a color with transparent in oklab keeps the
 * color and takes the mix's share as its opacity
 */
const selectedAlpha = (() => {
  const mix =
    /\[aria-selected="true"\] \{\s*background-color: color-mix\(in oklab, var\(--color-accent-soft\) ([\d.]+)%, transparent\);/.exec(
      rule("@utility line-tint")
    );
  if (!mix) throw new Error("line-tint's selected row is not a mix of accent-soft and transparent");
  return Number(mix[1]) / 100;
})();

/** a line-tint row's backgrounds: plain, hovered, selected, and selected under the pointer */
const rows = (block: string) => {
  const surface = shown(token(block, "surface"));
  const soft = shown(token(block, "accent-soft"));
  const selected = over(soft, selectedAlpha, surface);
  return [surface, shown(token(block, "hover")), selected, soft].map(luminance);
};

describe("username colors", () => {
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

  it("read as text on the rows of both themes", () => {
    const light = rows(rule("@theme"));
    const dark = rows(rule(".dark"));
    for (const h of NAME_HUES) {
      const onLight = luminance(shown([LIGHT.l, LIGHT.c, h]));
      const onDark = luminance(shown([DARK.l, DARK.c, h]));
      for (const surface of light) expect(contrast(onLight, surface)).toBeGreaterThanOrEqual(4.5);
      for (const surface of dark) expect(contrast(onDark, surface)).toBeGreaterThanOrEqual(4.5);
    }
  });
});
