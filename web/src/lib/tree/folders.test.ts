import { describe, expect, it } from "vitest";

import { Expansion } from "./expansion.svelte";
import { folderTree, treeLines } from "./folders";

type File = { filename: string; size: number };
const file = (filename: string, size = 10): File => ({ filename, size });
const tree = (files: File[]) =>
  folderTree(
    files,
    (f) => f.filename,
    (f) => f.size
  );

describe("folder trees", () => {
  it("nest folders under the chain they share, counting what is below", () => {
    const roots = tree([
      file("@@a\\Music\\Artist\\One\\1.flac", 1),
      file("@@a\\Music\\Artist\\One\\2.flac", 2),
      file("@@a\\Music\\Artist\\Two\\CD1\\1.flac", 4),
      file("@@a\\Music\\Artist\\cover.jpg", 8)
    ]);
    expect(roots).toHaveLength(1);
    const [top] = roots;
    expect(top).toMatchObject({ label: "@@a\\Music\\Artist", path: "@@a\\Music\\Artist" });
    expect(top.items.map((f) => f.filename)).toEqual(["@@a\\Music\\Artist\\cover.jpg"]);
    expect(top.children.map((c) => [c.label, c.path, c.count, c.bytes])).toEqual([
      ["One", "@@a\\Music\\Artist\\One", 2, 3],
      ["Two\\CD1", "@@a\\Music\\Artist\\Two\\CD1", 1, 4]
    ]);
    expect([top.count, top.bytes]).toEqual([4, 15]);
  });

  it("take forward slashes, and a file with no folder as its own root", () => {
    const roots = tree([file("music/a/1.mp3"), file("music/b/2.mp3"), file("loose.mp3")]);
    expect(roots.map((r) => r.label)).toEqual(["music", "loose.mp3"]);
    expect(roots[0].children.map((c) => c.path)).toEqual(["music/a", "music/b"]);
  });

  it("lay out open folders' files and folders, and only their headers when closed", () => {
    const roots = tree([file("x\\a\\1"), file("x\\a\\2"), file("x\\b\\3")]);
    const open = new Expansion(true);
    const keys = () =>
      treeLines(roots, open.isOpen, (f) => f.filename).map((l) => `${l.depth}:${l.key}`);
    expect(keys()).toEqual([
      "0:folder:x",
      "1:folder:x\\a",
      "2:x\\a\\1",
      "2:x\\a\\2",
      "1:folder:x\\b",
      "2:x\\b\\3"
    ]);
    open.toggle("x\\a");
    expect(keys()).toEqual(["0:folder:x", "1:folder:x\\a", "1:folder:x\\b", "2:x\\b\\3"]);
    open.toggle("x");
    expect(keys()).toEqual(["0:folder:x"]);
  });
});

describe("expansion", () => {
  it("toggles one folder, or with Alt every sibling to the state it takes", () => {
    const open = new Expansion(false);
    const siblings = ["a", "b", "c"];
    open.toggle("b", siblings);
    expect(siblings.map(open.isOpen)).toEqual([false, true, false]);
    // Alt on an open one closes them all, on a closed one opens them all
    open.toggle("b", siblings, true);
    expect(siblings.map(open.isOpen)).toEqual([false, false, false]);
    open.toggle("a", siblings, true);
    expect(siblings.map(open.isOpen)).toEqual([true, true, true]);
    open.toggle("c");
    expect(siblings.map(open.isOpen)).toEqual([true, true, false]);
  });

  it("goes back to one state for every key on a reset", () => {
    const open = new Expansion(true);
    open.toggle("a");
    expect(open.isOpen("a")).toBe(false);
    open.reset(false);
    expect([open.isOpen("a"), open.isOpen("z")]).toEqual([false, false]);
    open.reset(true);
    expect(open.isOpen("a")).toBe(true);
  });
});
