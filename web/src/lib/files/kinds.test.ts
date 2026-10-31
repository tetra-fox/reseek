import { describe, expect, it } from "vitest";

import { audioType, kindOf, needsWav } from "./kinds";

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
    expect([kindOf("old.wma"), audioType("old.wma"), audioType("x.mp3")]).toEqual([
      "audio",
      "",
      "audio/mpeg"
    ]);
  });

  it("play AIFF as a WAV, and an .m4a only when an analysis found ALAC in it", () => {
    expect([needsWav("a.aif"), needsWav("a.AIFF"), needsWav("a.aifc")]).toEqual([true, true, true]);
    expect(audioType("a.aiff")).toBe("audio/wav");
    expect([needsWav("a.m4a"), audioType("a.m4a")]).toEqual([false, "audio/mp4"]);
    expect([needsWav("a.m4a", "aac"), needsWav("a.m4a", "alac")]).toEqual([false, true]);
    expect(audioType("a.m4a", "alac")).toBe("audio/wav");
  });
});
