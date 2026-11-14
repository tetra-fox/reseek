import { describe, expect, it } from "vitest";

import fixture from "#lib/config/fixtures/view.json";
import { contrast, luminance, over, rule, shown, token } from "#lib/testing/contrast.js";

import { DARK, GROUP_COLORS, LIGHT, NAME_HUES, nameHue } from "./colors";

/**
 * How opaque a selected row's accent-soft is: mixing a color with transparent in oklab keeps the
 * color and takes the mix's share as its opacity
 */
const selectedAlpha = (() => {
  const mix =
    /\[aria-selected="true"\] \{\s*background-color: color-mix\(\s*in oklab,\s*var\(--color-accent-soft\) ([\d.]+)%,\s*var\(--line-under, transparent\)\s*\);/.exec(
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

describe("group colors", () => {
  it("are every color the daemon offers, in its order", () => {
    const entry = fixture.fields["transfers.groups.user_defined"].entries.find(
      (e) => e.key === "color"
    );
    expect(GROUP_COLORS).toEqual(entry?.choices);
  });
});
