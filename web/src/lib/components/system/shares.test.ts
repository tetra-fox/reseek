import { describe, expect, it } from "vitest";

import { contentsText } from "./shares";

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
