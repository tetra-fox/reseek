import { describe, expect, it } from "vitest";

import { audioType, kindOf, needsWav } from "./kinds";

describe("file kinds", () => {
  it("know a file by its extension, in any case, and what the browser plays", () => {
    expect([kindOf("a\\b.FLAC"), kindOf("cover.jpg")]).toEqual(["audio", "other"]);
    // audio a browser can't play still has the audio icon
    expect([kindOf("old.wma"), audioType("old.wma")]).toEqual(["audio", ""]);
  });

  it("play AIFF as a WAV, and an .m4a only when an analysis found ALAC in it", () => {
    expect([needsWav("a.AIFF"), audioType("a.aiff")]).toEqual([true, "audio/wav"]);
    expect([needsWav("a.m4a", "aac"), needsWav("a.m4a", "alac")]).toEqual([false, true]);
    expect(audioType("a.m4a", "alac")).toBe("audio/wav");
  });
});
