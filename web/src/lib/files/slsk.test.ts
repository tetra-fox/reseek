import { describe, expect, it } from "vitest";

import { parseSlsk, slskBrowse, slskLink } from "./slsk";

describe("slsk:// links", () => {
  it("write a file's path with slashes, each part encoded, and a folder with a slash after", () => {
    expect(slskLink("al ice", "@@x\\Music\\Album #1\\01 One.flac")).toBe(
      "slsk://al%20ice/%40%40x/Music/Album%20%231/01%20One.flac"
    );
    expect(slskLink("bob", "Music\\Album", true)).toBe("slsk://bob/Music/Album/");
  });

  it("read back what they write, a folder told from a file", () => {
    for (const [username, path, folder] of [
      ["al ice", "@@x\\Music\\Album #1\\01 One.flac", false],
      ["бob/2", "Музыка\\100% hits\\a.mp3", false],
      ["bob", "Music\\Album", true]
    ] as const) {
      expect(parseSlsk(slskLink(username, path, folder))).toEqual({ username, path, folder });
    }
  });

  it("take links written by other clients and leave other text alone", () => {
    expect(parseSlsk("  SLSK://carol/Music/a%2Fb.mp3 ")).toEqual({
      username: "carol",
      path: "Music\\a/b.mp3",
      folder: false
    });
    expect(parseSlsk("slsk://dave/50%off/x")).toEqual({
      username: "dave",
      path: "50%off\\x",
      folder: false
    });
    expect(parseSlsk("slsk://erin/")).toBeNull();
    expect(parseSlsk("slsk://erin")).toBeNull();
    expect(parseSlsk("https://example.com/a")).toBeNull();
    expect(parseSlsk("slsk://a b/c")).toBeNull();
  });

  it("open the folder in Browse", () => {
    expect(slskBrowse({ username: "al ice", path: "Music\\Album\\1.flac", folder: false })).toBe(
      "#/browse?user=al%20ice&path=Music%5CAlbum"
    );
    expect(slskBrowse({ username: "bob", path: "Music\\Album", folder: true })).toBe(
      "#/browse?user=bob&path=Music%5CAlbum"
    );
  });
});
