import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import type { ViewSettings } from "./settings.svelte";
import { activeFilters, layout, textFilter } from "./view";

describe("the transfers view", () => {
  it("filters by user, folder, extension and words", () => {
    const keep = textFilter("user:ali folder:album ext:flac one")!;
    expect(keep(transfer())).toBe(true);
    expect(keep(transfer({ username: "bob" }))).toBe(false);
    expect(keep(transfer({ filename: "@@a\\Music\\Album\\01 One.mp3" }))).toBe(false);
    expect(textFilter("  ")).toBeNull();
  });

  it("takes the file filter's sizes, types and excluded words, and negated tokens", () => {
    const big = transfer({ size: 50 * 1024 ** 2 });
    const small = transfer({ size: 1024 });
    expect([big, small].map(textFilter("size:>10m")!)).toEqual([true, false]);
    expect(textFilter("-one")!(transfer())).toBe(false);
    expect(textFilter("type:mp3|flac")!(transfer())).toBe(true);
    expect(textFilter("-user:ali")!(transfer())).toBe(false);
    // a transfer says nothing of its bit rate, which never fails a bit rate token
    expect(textFilter("bitrate:320")!(transfer())).toBe(true);
  });

  it("filters by analysis through the marks it is given", () => {
    const mark = {
      verdict: "consistent",
      profile: null,
      judgment: "fake",
      note: null,
      stale: false
    } as const;
    const marks = (t: { id: string }) => ({
      mark: t.id === "marked" ? mark : undefined,
      analyzable: true
    });
    const fake = textFilter("marked:fake", marks)!;
    expect(fake(transfer({ id: "marked" }))).toBe(true);
    expect(fake(transfer({ id: "other" }))).toBe(false);
    expect(textFilter("analyzed:no one", marks)!(transfer({ id: "other" }))).toBe(true);
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

  it("groups by what was queued together, named by its file, its folder or its users", () => {
    const disc1 = transfer({ jobId: "j1", filename: "@@a\\Music\\Album\\CD1\\1.flac" });
    const other = transfer({ jobId: "j2", filename: "@@a\\Music\\Single.flac" });
    const disc2 = transfer({ jobId: "j1", filename: "@@a\\Music\\Album\\CD2\\1.flac" });
    const top = transfer({ jobId: "j3", filename: "@@a\\1.flac" });
    const second = transfer({ jobId: "j3", filename: "@@a\\2.flac" });
    const mixed = transfer({ jobId: "j4", filename: "@@a\\Album\\1.flac" });
    const bobs = transfer({ jobId: "j4", username: "bob", filename: "@@b\\Album\\2.flac" });
    const old = transfer({ filename: "@@a\\old.flac" });
    const groups = layout(
      [disc1, other, disc2, top, second, mixed, bobs, old],
      "job",
      () => true
    ).flatMap((i) => (i.type === "group" ? [i.group] : []));
    expect(groups.map((g) => [g.key, g.label, g.transfers.map((t) => t.id)])).toEqual([
      ["job:j1", "Album from alice", [disc1.id, disc2.id]],
      ["job:j2", "Single.flac from alice", [other.id]],
      ["job:j3", "Downloads from alice", [top.id, second.id]],
      ["job:j4", "Downloads from alice and bob", [mixed.id, bobs.id]],
      ["job:none", "Other downloads", [old.id]]
    ]);
    expect(groups[0].siblings).toHaveLength(5);
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

  it("names each filter on and takes each off alone", () => {
    const filters: ViewSettings["filters"] = { states: ["errored", "active"], analysis: "fake" };
    let text = " flac ";
    const on = () => activeFilters(filters, text, () => (text = ""));
    // in the order the chips stand in, then the analysis, then the text
    expect(on().map((f) => f.label)).toEqual([
      "Active",
      "Failed",
      "Marked fake",
      'Matching "flac"'
    ]);
    on()[1].clear();
    expect(filters.states).toEqual(["active"]);
    on()[1].clear();
    expect(filters.analysis).toBeNull();
    on()[1].clear();
    expect(text).toBe("");
    expect(on().map((f) => f.id)).toEqual(["state:active"]);
  });
});
