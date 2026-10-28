import { describe, expect, it } from "vitest";

import { paletteScore, paletteValue } from "./palette";

describe("the command palette's matching", () => {
  const settings = ["Web", "web.port web.https.port web.authentication.api_keys"];

  it("matches a label loosely and keywords only as a whole", () => {
    const web = paletteValue("Web", "setting:web");
    expect(paletteScore(web, "wb", settings)).toBeGreaterThan(0);
    expect(paletteScore(web, "https", settings)).toBeGreaterThan(0);
    // letters scattered over the option names are not a match
    expect(paletteScore(web, "r1b", settings)).toBe(0);
    expect(paletteScore(web, "ptas", settings)).toBe(0);
  });

  it("ranks a label above a keyword, and the typed actions below both", () => {
    const downloads = paletteScore(paletteValue("Downloads", "/downloads"), "down");
    const keyword = paletteScore(paletteValue("Shares", "setting:shares"), "down", [
      "shares.downloads"
    ]);
    const action = paletteScore(paletteValue("Search for down", "do:search"), "down");
    expect(downloads).toBeGreaterThan(keyword);
    expect(keyword).toBeGreaterThan(action);
    expect(action).toBeGreaterThan(0);
  });
});
