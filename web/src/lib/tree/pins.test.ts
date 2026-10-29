import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";
import { depthOf, layout } from "#lib/transfers/grid/view.js";

import { nesting, pins } from "./pins";

// alice > M > A, six files and the subfolder S of six more, then M > B of six: A's header is
// line 2, S's line 9, B's line 16, and the list is 23 lines
const files = ["A", "A\\S", "B"].flatMap((folder) =>
  Array.from({ length: 6 }, (_, n) => transfer({ filename: `M\\${folder}\\${n}.flac` }))
);
const items = layout(files, "user", () => true);
const nest = nesting(items, depthOf, (item) => item.type === "group");
const ROW = 10;
const wide = (depth: number) => depth;
const phone = (depth: number) => Math.min(depth, 1);
/** the pins as `label@top`, in painting order */
const at = (scrollTop: number, slot = wide) =>
  pins(nest, scrollTop, ROW, slot).map(({ index, top }) => {
    const item = items[index];
    return `${item.type === "group" ? item.group.label : item.key}@${top}`;
  });

describe("the pinned headers", () => {
  it("knows where each group ends and what each line is in", () => {
    expect(items).toHaveLength(23);
    expect([nest.end[0], nest.end[1], nest.end[2], nest.end[9], nest.end[16]]).toEqual([
      23, 23, 16, 16, 23
    ]);
    expect([nest.parent[3], nest.parent[10], nest.parent[16], nest.parent[0]]).toEqual([
      2, 9, 1, -1
    ]);
    expect(nest.deepest).toBe(3);
  });

  it("pins nothing while the first header is in its place", () => {
    expect(at(0)).toEqual([]);
  });

  it("stacks the whole chain in a deep folder, the outer ones painted over the inner", () => {
    expect(at(110)).toEqual(["S@30", "A@20", "M@10", "alice@0"]);
  });

  it("pushes a folder's header up as its last line passes, then the next folder takes its place", () => {
    // S and A end together, so A is pushed over S, which it covers
    expect(at(135)).toEqual(["A@15", "M@10", "alice@0"]);
    expect(at(150)).toEqual(["B@20", "M@10", "alice@0"]);
  });

  it("keeps the top group and the innermost folder on a phone", () => {
    expect(at(110, phone)).toEqual(["S@10", "alice@0"]);
    // S ends with A, so as its end pushes it out, M shows behind it
    expect(at(145, phone)).toEqual(["M@10", "S@5", "alice@0"]);
  });
});
