import { describe, expect, it } from "vitest";

import type { Schemas } from "#lib/api/client.js";

import { Expansion } from "./expansion.svelte";
import {
  ancestors,
  detectSeparator,
  folderTree,
  itemsUnder,
  listingTree,
  type RemoteDirectory,
  tickedCounts,
  treeAt,
  treeLines
} from "./folders";

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

  it("take forward slashes, and put files with no folder in a node with no path, last", () => {
    const roots = tree([file("loose.mp3"), file("music/a/1.mp3"), file("music/b/2.mp3")]);
    expect(roots.map((r) => [r.label, r.path])).toEqual([
      ["music", "music"],
      ["", ""]
    ]);
    expect(roots[0].children.map((c) => c.path)).toEqual(["music/a", "music/b"]);
    expect(roots[1].items.map((f) => f.filename)).toEqual(["loose.mp3"]);
  });

  it("keep a Unix or macOS path's leading slash in its folders' paths and labels", () => {
    const [unix] = tree([file("/home/ryu/Music/a.flac"), file("/home/ryu/Music/b.flac")]);
    expect([unix.label, unix.path]).toEqual(["/home/ryu/Music", "/home/ryu/Music"]);
    const [mac] = tree([
      file("/Volumes/Untitled 2/Music/Sigur Rós/( )/1.m4a"),
      file("/Volumes/Untitled 2/Music/Sigur Rós/Takk/2.m4a")
    ]);
    expect([mac.label, mac.path]).toEqual([
      "/Volumes/Untitled 2/Music/Sigur Rós",
      "/Volumes/Untitled 2/Music/Sigur Rós"
    ]);
    expect(mac.children.map((c) => [c.label, c.path])).toEqual([
      ["( )", "/Volumes/Untitled 2/Music/Sigur Rós/( )"],
      ["Takk", "/Volumes/Untitled 2/Music/Sigur Rós/Takk"]
    ]);
    // the top level's folders sit side by side, and a file at the root is in "/"
    const roots = tree([file("/home/a/1"), file("/mnt/b/2"), file("/3")]);
    expect(roots.map((r) => r.path)).toEqual(["/home/a", "/mnt/b", "/"]);
  });

  it("take Windows drive and network paths whole", () => {
    const [drive] = tree([file("C:\\Users\\Bart\\x\\1.mp3"), file("C:\\Users\\Bart\\y\\2.mp3")]);
    expect([drive.label, drive.path]).toEqual(["C:\\Users\\Bart", "C:\\Users\\Bart"]);
    expect(drive.children.map((c) => c.path)).toEqual(["C:\\Users\\Bart\\x", "C:\\Users\\Bart\\y"]);
    const [unc] = tree([file("\\\\nas\\music\\a\\1.flac")]);
    expect([unc.label, unc.path]).toEqual(["\\\\nas\\music\\a", "\\\\nas\\music\\a"]);
  });

  it("put files with no folder in the line above the tree, at the user's level", () => {
    const at = { key: "user:0:jo", path: "jo", label: "jo" };
    const user = treeAt(
      at,
      [file("@@jo\\Music\\1.flac", 1), file("bare name.flac", 2), file("/srv/x/2.flac", 4)],
      (f) => f.filename,
      (f) => f.size
    );
    expect(user.items.map((f) => f.filename)).toEqual(["bare name.flac"]);
    expect(user.children.map((c) => c.path)).toEqual(["@@jo\\Music", "/srv/x"]);
    expect([user.key, user.count, user.bytes, user.folders]).toEqual(["user:0:jo", 3, 7, 2]);
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

const remote = (filename: string, size = 10): Schemas["File"] => ({
  filename,
  size,
  code: 1,
  extension: "",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

const dir = (name: string, files: string[] = [], locked = false): RemoteDirectory => ({
  name,
  fileCount: files.length,
  files: files.map((f) => remote(f)),
  locked
});

describe("browse listings", () => {
  it("find the separator from the first folder name with one", () => {
    expect(detectSeparator([{ name: "top" }, { name: "a/b" }, { name: "c\\d" }])).toBe("/");
    expect(detectSeparator([{ name: "top" }])).toBe("\\");
  });

  it("root a folder whose parent isn't listed and count what is under each", () => {
    const listing = listingTree(
      [
        dir("C:\\Music\\A", ["1.mp3"]),
        dir("C:\\Music\\A\\Disc 1", ["2.flac", "1.flac"]),
        dir("C:\\Music\\A\\Disc 1\\Scans", ["cover.jpg"], true),
        dir("C:\\Other", [])
      ],
      "\\"
    );
    expect(listing.roots.map((r) => [r.path, r.label])).toEqual([
      ["C:\\Music\\A", "A"],
      ["C:\\Other", "Other"]
    ]);
    const [a] = listing.roots;
    expect([a.count, a.folders, a.bytes]).toEqual([4, 2, 40]);
    expect(a.children[0].children[0].locked).toBe(true);
    // a folder's files carry their full paths, sorted by name
    expect(a.children[0].items.map((f) => f.filename)).toEqual([
      "C:\\Music\\A\\Disc 1\\1.flac",
      "C:\\Music\\A\\Disc 1\\2.flac"
    ]);
    expect(itemsUnder(a)).toHaveLength(4);
    expect(ancestors("C:\\Music\\A\\Disc 1\\Scans", listing, "\\")).toEqual([
      "C:\\Music\\A",
      "C:\\Music\\A\\Disc 1"
    ]);
  });

  it("keep a file's figures that can't be true as unknown, as a search does", () => {
    const wrapped = 4_294_967_295;
    const odd = { ...remote("odd.flac"), length: wrapped, bitRate: wrapped, sampleRate: wrapped };
    const fine = { ...remote("fine.flac"), length: 300, bitRate: 0, sampleRate: 44_100 };
    const [root] = listingTree([{ name: "a", fileCount: 2, files: [odd, fine] }], "\\").roots;
    expect(root.items.map((f) => [f.length, f.bitRate, f.sampleRate])).toEqual([
      [300, 0, 44_100],
      [null, null, null]
    ]);
  });

  it("show every way to a match when filtering, open, with the items kept", () => {
    const { roots } = listingTree(
      [dir("a"), dir("a\\b"), dir("a\\b\\target", ["x.flac", "y.mp3"]), dir("a\\c"), dir("d")],
      "\\"
    );
    const lines = treeLines(
      roots,
      () => false,
      (f) => f.filename,
      { folder: (n) => n.label === "target", item: (f) => f.filename.endsWith(".flac") }
    );
    expect(lines.map((l) => `${l.depth}:${l.key}`)).toEqual([
      "0:folder:a",
      "1:folder:a\\b",
      "2:folder:a\\b\\target",
      "3:a\\b\\target\\x.flac"
    ]);
  });
});

describe("ticked counts", () => {
  it("count each ticked file once in every folder above it", () => {
    const counts = tickedCounts(["a\\b\\1", "a\\b\\2", "a\\c\\3", "x/4"]);
    expect(Object.fromEntries(counts)).toEqual({ "a": 3, "a\\b": 2, "a\\c": 1, "x": 1 });
  });

  it("count under the scoped keys when the scope holds a slash", () => {
    const counts = tickedCounts(["0:ac/dc\u0000@@abc\\Music\\x.mp3"]);
    expect(Object.fromEntries(counts)).toEqual({
      "0:ac/dc\u0000@@abc": 1,
      "0:ac/dc\u0000@@abc\\Music": 1
    });
  });
});

describe("expansion", () => {
  it("toggles one folder, or with others taking the state it takes", () => {
    const open = new Expansion(false);
    const siblings = ["a", "b", "c"];
    open.toggle("b");
    expect(siblings.map(open.isOpen)).toEqual([false, true, false]);
    // on an open one the others close with it, on a closed one they open with it
    open.toggle("b", ["a", "c"]);
    expect(siblings.map(open.isOpen)).toEqual([false, false, false]);
    open.toggle("a", ["b", "c"]);
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
