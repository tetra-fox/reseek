import { describe, expect, it } from "vitest";

import { contrast, luminance, rows, rule, shown, token } from "#lib/testing/contrast.js";

// the colour tokens of app.css
describe("status text", () => {
  it("reads at 4.5:1 on every surface text sits on, in both themes", () => {
    for (const block of [rule("@theme"), rule(".dark")]) {
      for (const text of ["success-text", "warning-text", "danger"]) {
        const fg = luminance(shown(token(block, text)));
        for (const under of ["surface", "page", "surface-muted", "overlay", "hover"]) {
          const ratio = contrast(fg, luminance(shown(token(block, under))));
          expect(ratio, `${text} on ${under}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
  });
});

describe("status icons", () => {
  it("reach 3:1 on every surface and row an icon sits on, in both themes", () => {
    for (const [theme, block] of [
      ["light", rule("@theme")],
      ["dark", rule(".dark")]
    ]) {
      // press is a control's tint inside a hovered line, and a sidebar link's while pressed
      const under: Record<string, number> = { ...rows(block) };
      for (const name of ["page", "surface-muted", "overlay", "press", "warning-soft"]) {
        under[name] = luminance(shown(token(block, name)));
      }
      for (const icon of ["success-icon", "warning-icon", "danger"]) {
        const fg = luminance(shown(token(block, icon)));
        for (const [name, bg] of Object.entries(under)) {
          expect(contrast(fg, bg), `${theme} ${icon} on ${name}`).toBeGreaterThanOrEqual(3);
        }
      }
    }
  });
});
