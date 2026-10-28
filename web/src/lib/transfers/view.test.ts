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

  it("filters by analysis through the marks it is given", () => {
    const mark = { verdict: "consistent", profile: null, judgement: "fake", note: null } as const;
    const marks = (t: { id: string }) => ({
      mark: t.id === "marked" ? mark : undefined,
      analysable: true
    });
    const fake = textFilter("marked:fake", marks)!;
    expect(fake(transfer({ id: "marked" }))).toBe(true);
    expect(fake(transfer({ id: "other" }))).toBe(false);
    expect(textFilter("analysed:no one", marks)!(transfer({ id: "other" }))).toBe(true);
    // the uploads screen has no marks, so an analysis word matches nothing there
    expect(textFilter("marked:fake")!(transfer({ id: "marked" }))).toBe(false);
  });

  it("groups by user then a tree of folders, and closes a group to its header", () => {
    const a = transfer({ filename: "x\\A\\1.flac" });
    const b = transfer({ filename: "x\\B\\1.flac" });
    const c = transfer({ username: "bob", filename: "y\\1.flac" });
    const lines = (closed: string[] = []) =>
      layout([a, b, c], "user", (key) => !closed.includes(key)).map((i) =>
        i.type === "group" ? `${i.group.depth}:${i.group.label}` : i.key
      );
    expect(lines()).toEqual(["0:alice", "1:x", "2:A", a.id, "2:B", b.id, "0:bob", "1:y", c.id]);
    expect(lines(["user:alice"])).toEqual(["0:alice", "0:bob", "1:y", c.id]);
    expect(lines(["folder:alice\u0000x\\A"])).toEqual([
      "0:alice",
      "1:x",
      "2:A",
      "2:B",
      b.id,
      "0:bob",
      "1:y",
      c.id
    ]);
    expect(layout([a, c], "flat", () => true).map((i) => i.key)).toEqual([a.id, c.id]);
  });

  it("gives a folder every transfer below it, and its siblings for Alt-click", () => {
    const a = transfer({ filename: "x\\A\\1.flac" });
    const b = transfer({ filename: "x\\B\\1.flac" });
    const groups = layout([a, b], "folder", () => true).flatMap((i) =>
      i.type === "group" ? [i.group] : []
    );
    expect(groups.map((g) => [g.label, g.transfers.length, g.siblings.length])).toEqual([
      ["x", 2, 1],
      ["A", 1, 2],
      ["B", 1, 2]
    ]);
    expect(groups[1].folder).toBe("x\\A");
  });
});
