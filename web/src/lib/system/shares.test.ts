import { describe, expect, it } from "vitest";

import {
  addition,
  aliasOf,
  contentsText,
  entryAlias,
  entryPath,
  shareEntry,
  takenAliases
} from "./shares";

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

describe("share aliases", () => {
  it("are read from an entry as the daemon reads them", () => {
    expect(entryAlias("[Music]/srv/music")).toBe("Music");
    expect(entryAlias("!/srv/music/private/")).toBe("private");
    expect(entryAlias("-[Old]C:\\old")).toBe("Old");
    expect(entryAlias("C:\\Users\\me\\Rare")).toBe("Rare");
    expect(aliasOf("/srv/rare/", " ")).toBe("rare");
  });

  it("taken include the downloads directory only when it is shared and listed by no entry", () => {
    const entries = ["[Music]/srv/music", "!/srv/music/private"];
    expect([...takenAliases(entries, false, null)]).toEqual(["music", "private"]);
    expect(takenAliases(entries, true, null).has("downloads")).toBe(true);
    expect(takenAliases(entries, true, "/data/got/").has("got")).toBe(true);
    const listed = [...entries, "[Fresh]/data/got"];
    expect(takenAliases(listed, true, "/data/got").has("got")).toBe(false);
  });
});

describe("adding a share", () => {
  it("writes who sees it before the share, and only when that changes", () => {
    const set = addition(["/srv/a"], "[b]/srv/b", { a: [] }, []);
    expect(Object.keys(set)).toEqual(["shares.visible_to", "shares.directories"]);
    expect(set).toEqual({
      "shares.visible_to": { a: [], b: [] },
      "shares.directories": ["/srv/a", "[b]/srv/b"]
    });
    expect(Object.keys(addition([], "/srv/b", {}, undefined))).toEqual(["shares.directories"]);
  });
});
