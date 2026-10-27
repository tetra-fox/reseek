import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import { layout, textFilter } from "./view";

describe("the transfers view", () => {
  it("filters by user, folder, extension and words", () => {
    const keep = textFilter("user:ali folder:album ext:flac one")!;
    expect(keep(transfer())).toBe(true);
    expect(keep(transfer({ username: "bob" }))).toBe(false);
    expect(keep(transfer({ filename: "@@a\\Music\\Album\\01 One.mp3" }))).toBe(false);
    expect(textFilter("  ")).toBeNull();
  });

  it("groups by user then folder, and folds a group to its header", () => {
    const a = transfer({ filename: "x\\A\\1.flac" });
    const b = transfer({ filename: "x\\B\\1.flac" });
    const c = transfer({ username: "bob", filename: "y\\1.flac" });
    const lines = (folded: Set<string>) =>
      layout([a, b, c], "user", folded).map((i) => (i.type === "group" ? i.group.label : i.key));
    expect(lines(new Set())).toEqual(["alice", "x\\A", a.id, "x\\B", b.id, "bob", "y", c.id]);
    expect(lines(new Set(["user:alice"]))).toEqual(["alice", "bob", "y", c.id]);
    expect(layout([a, c], "flat", new Set()).map((i) => i.key)).toEqual([a.id, c.id]);
  });
});
