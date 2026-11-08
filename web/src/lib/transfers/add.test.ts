import { describe, expect, it } from "vitest";

import { parsePaste } from "./add";

describe("the add dialog's paste box", () => {
  it("reads slsk links and the JSON of files or search responses", () => {
    const links = parsePaste(
      "slsk://al%20ice/Music/Album/01.flac\nnot a link\n\nslsk://bob/Music/"
    );
    expect([...links.wanted]).toEqual([
      ["al ice", [{ filename: "Music\\Album\\01.flac", size: 0 }]]
    ]);
    // a line that is not a link and a link to a folder are counted, blank lines are not
    expect(links.unread).toBe(2);
    expect([
      ...parsePaste(
        JSON.stringify([
          { username: "bob", filename: "a\\1.mp3", size: 5 },
          { username: "bob", files: [{ filename: "a\\2.mp3", size: 7 }], lockedFiles: [] }
        ])
      ).wanted
    ]).toEqual([
      [
        "bob",
        [
          { filename: "a\\1.mp3", size: 5 },
          { filename: "a\\2.mp3", size: 7 }
        ]
      ]
    ]);
    expect(parsePaste("{ broken\n}")).toEqual({ wanted: new Map(), unread: 2 });
  });
});
