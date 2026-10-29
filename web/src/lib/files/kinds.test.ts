import { describe, expect, it } from "vitest";

import { audioType, isAudio, kindOf } from "./kinds";

describe("file kinds", () => {
  it("know a file by its extension, in any case, and what the browser plays", () => {
    expect([kindOf("a\\b.FLAC"), kindOf("x.mkv"), kindOf("cover.jpg"), kindOf("noext")]).toEqual([
      "audio",
      "video",
      "other",
      "other"
    ]);
    expect(audioType("song.opus")).toBe("audio/ogg; codecs=opus");
    // audio a browser can't play still has the audio icon
    expect([kindOf("old.wma"), isAudio("old.wma"), isAudio("x.mp3")]).toEqual([
      "audio",
      false,
      true
    ]);
  });
});
