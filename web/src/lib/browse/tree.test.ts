import { describe, expect, it } from "vitest";

import type { Schemas } from "#lib/api/client.js";

import {
  buildTree,
  detectSeparator,
  downloadGroups,
  filesUnder,
  flatten,
  type RemoteDirectory
} from "./tree";

const file = (filename: string, size = 10): Schemas["File"] => ({
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
  files: files.map((f) => file(f)),
  locked
});

describe("the browse tree", () => {
  it("finds the separator from the first folder name with one", () => {
    expect(detectSeparator([{ name: "top" }, { name: "a/b" }, { name: "c\\d" }])).toBe("/");
    expect(detectSeparator([{ name: "top" }])).toBe("\\");
  });

  it("roots a folder whose parent isn't listed and counts what is under each", () => {
    const tree = buildTree(
      [
        dir("C:\\Music\\A", ["1.mp3"]),
        dir("C:\\Music\\A\\Disc 1", ["1.flac", "2.flac"]),
        dir("C:\\Music\\A\\Disc 1\\Scans", ["cover.jpg"], true),
        dir("C:\\Other", [])
      ],
      "\\"
    );
    expect(tree.roots.map((r) => r.name)).toEqual(["C:\\Music\\A", "C:\\Other"]);
    const a = tree.roots[0];
    expect(a.totalFileCount).toBe(4);
    expect(a.totalDirectoryCount).toBe(2);
    expect(a.totalBytes).toBe(40);
    expect(a.children[0].children[0].locked).toBe(true);
  });

  it("lists expanded folders depth first, or every way to a match when filtering", () => {
    const tree = buildTree(
      [dir("a"), dir("a\\b"), dir("a\\b\\target"), dir("a\\c"), dir("d")],
      "\\"
    );
    const names = (rows: ReturnType<typeof flatten>) =>
      rows.map((r) => `${r.depth}:${r.node.name}`);
    expect(names(flatten(tree.roots, new Set()))).toEqual(["0:a", "0:d"]);
    expect(names(flatten(tree.roots, new Set(["a"])))).toEqual(["0:a", "1:a\\b", "1:a\\c", "0:d"]);
    expect(names(flatten(tree.roots, new Set(), (n) => n.name.endsWith("target")))).toEqual([
      "0:a",
      "1:a\\b",
      "2:a\\b\\target"
    ]);
  });

  it("gives every file under a folder its full path", () => {
    const tree = buildTree([dir("a", ["1"]), dir("a\\b", ["2"])], "\\");
    expect(filesUnder(tree.roots[0], "\\").map((f) => f.filename)).toEqual(["a\\1", "a\\b\\2"]);
  });

  it("splits a download into the folder's own files and one batch per subfolder", () => {
    const groups = downloadGroups(
      "C:\\Music\\Album",
      [
        file("C:\\Music\\Album\\Disc 1\\Extras\\x.mp3"),
        file("C:\\Music\\Album\\cover.jpg"),
        file("C:\\Music\\Album\\Disc 1\\01.mp3"),
        file("C:\\Music\\Album\\Disc 2\\01.mp3")
      ],
      "\\"
    );
    expect(groups.map((g) => [g.destination, g.files.length])).toEqual([
      [null, 1],
      ["Album/Disc 1", 2],
      ["Album/Disc 2", 1]
    ]);
    expect(downloadGroups("top", [file("top/sub/a")], "/")[0].destination).toBe("top/sub");
  });
});
