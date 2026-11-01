import { describe, expect, it } from "vitest";

import { route } from "#lib/nav.js";

import { analysisHref, parseTarget } from "./address";
import type { Target } from "./analysis";

/** The slots a link restores, as the page reads them from its address */
const restored = (link: string) => {
  const { path, query } = route({ hash: link });
  return { path, a: parseTarget(query.get("a")), b: parseTarget(query.get("b")) };
};

describe("the Analysis page's address", () => {
  it("restores two slots, whatever a path or a name holds", () => {
    const a: Target = { kind: "file", root: "downloads", path: "Album: Live?/01 & 02 #1.flac" };
    const b: Target = { kind: "download", username: "a:b c", id: "2f1ac000-0000" };
    expect(restored(analysisHref(a, b))).toEqual({ path: "/analysis", a, b });
    expect(restored(analysisHref(null, b))).toEqual({ path: "/analysis", a: null, b });
  });

  it("holds no local file", () => {
    const local: Target = { kind: "file", root: "temporary", path: "id/a.flac" };
    const b: Target = { kind: "file", root: "incomplete", path: "b.flac" };
    expect(analysisHref(local, null)).toBe("#/analysis");
    expect(restored(analysisHref(local, b)).a).toBeNull();
    expect(parseTarget("temporary:id/a.flac")).toBeNull();
  });

  it("reads anything else as an empty slot", () => {
    for (const text of ["", "downloads:", "download:id", "download::bob", "download:id:", "x"]) {
      expect(parseTarget(text)).toBeNull();
    }
    expect(parseTarget(null)).toBeNull();
  });
});
