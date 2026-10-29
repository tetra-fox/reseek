import { describe, expect, it } from "vitest";

import type { Schemas } from "#lib/api/client.js";

import { downloadGroups } from "./tree";

const file = (filename: string, size = 10): Schemas["File"] => ({
  filename,
  size,
  code: 1,
  extension: "",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

describe("downloading a selection", () => {
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
