import { describe, expect, it } from "vitest";

import { contentsText, entryPath, shareEntry } from "./shares";

describe("a share's contents as text", () => {
  it("names directories by local path and lists their files sorted, without the share prefix", () => {
    const share = { remotePath: "music", localPath: "/srv/music" } as never;
    const text = contentsText(share, [
      {
        name: "music\\Album",
        fileCount: 2,
        files: [{ filename: "music\\Album\\02 b.flac" }, { filename: "music\\Album\\01 a.flac" }]
      }
    ] as never);
    expect(text).toBe("/srv/music\\Album\n\tAlbum\\01 a.flac\n\tAlbum\\02 b.flac\n");
  });
});

describe("share entries", () => {
  it("are written with an optional exclusion and alias", () => {
    expect(shareEntry("/music", "", false)).toBe("/music");
    expect(shareEntry("/music", " Tunes ", false)).toBe("[Tunes]/music");
    expect(shareEntry("/music/raw", "", true)).toBe("!/music/raw");
    expect(shareEntry("/music/raw", "x", true)).toBe("![x]/music/raw");
  });

  it("are read back to their paths, whichever way they are written", () => {
    expect(entryPath("/music/")).toBe("/music");
    expect(entryPath("[Tunes]/music")).toBe("/music");
    expect(entryPath("!/music")).toBe("/music");
    expect(entryPath("-[x]/music/[live]")).toBe("/music/[live]");
    expect(entryPath("D:\\Music\\")).toBe("D:\\Music");
  });
});
