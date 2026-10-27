import { describe, expect, it } from "vitest";

import { parsePaste } from "./add";

describe("the add dialog's paste box", () => {
  it("reads slsk links and the JSON of files or search responses", () => {
    expect([...parsePaste("slsk://al%20ice/Music/Album/01.flac\nnot a link")]).toEqual([
      ["al ice", [{ filename: "Music\\Album\\01.flac", size: 0 }]]
    ]);
    expect([
      ...parsePaste(
        JSON.stringify([
          { username: "bob", filename: "a\\1.mp3", size: 5 },
          { username: "bob", files: [{ filename: "a\\2.mp3", size: 7 }], lockedFiles: [] }
        ])
      )
    ]).toEqual([
      [
        "bob",
        [
          { filename: "a\\1.mp3", size: 5 },
          { filename: "a\\2.mp3", size: 7 }
        ]
      ]
    ]);
    expect(parsePaste("{ broken").size).toBe(0);
  });
});
