import { describe, expect, it } from "vitest";

import { contrast, luminance, rule, shown, token } from "#lib/testing/contrast.js";

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
