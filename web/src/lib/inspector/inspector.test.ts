import { describe, expect, it } from "vitest";

import { PANE_MIN, readPrefs } from "./inspector.svelte";

describe("the inspector's saved preferences", () => {
  it("fall back to the defaults on anything they can't use", () => {
    const defaults = {
      open: false,
      width: 384,
      sections: { details: true, analysis: true, history: true, technical: false }
    };
    expect(readPrefs(null)).toEqual(defaults);
    expect(readPrefs("nonsense")).toEqual(defaults);
    expect(readPrefs({ open: "yes", width: PANE_MIN - 1, sections: [true] })).toEqual(defaults);
    expect(readPrefs({ width: "wide", sections: { details: "open" } })).toEqual(defaults);
  });

  it("keep each section's own choice and leave the others at their defaults", () => {
    const prefs = readPrefs({
      open: true,
      width: 500,
      sections: { details: false, technical: true }
    });
    expect(prefs.open).toBe(true);
    expect(prefs.width).toBe(500);
    expect(prefs.sections).toEqual({
      details: false,
      analysis: true,
      history: true,
      technical: true
    });
  });
});
