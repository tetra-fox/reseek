import { describe, expect, it } from "vitest";

import { type Mark, markHint, markToken, passes } from "./marks";

const reading: Mark = {
  verdict: "likely_transcode",
  profile: "mp3 ~128 kbps",
  judgement: null,
  note: null
};
const fake: Mark = { ...reading, judgement: "fake", note: "an old rip" };

describe("filtering files by their analysis", () => {
  it("tells marked, analysed and not analysed files apart", () => {
    expect(passes("fake", fake, true)).toBe(true);
    expect(passes("fake", reading, true)).toBe(false);
    expect(passes("genuine", { ...reading, judgement: "legit" }, true)).toBe(true);
    expect(passes("analysed", reading, true)).toBe(true);
    expect(passes("analysed", undefined, true)).toBe(false);
    expect(passes("unanalysed", undefined, true)).toBe(true);
    // a cover image is not waiting to be analysed
    expect(passes("unanalysed", undefined, false)).toBe(false);
  });

  it("goes by the analyser's reading even when the user judged the file", () => {
    expect(passes("reading:likely_transcode", fake, true)).toBe(true);
    expect(passes("reading:consistent", fake, true)).toBe(false);
    expect(passes("reading:consistent", undefined, true)).toBe(false);
  });

  it("reads the filter language's analysis words and no others", () => {
    expect(markToken("marked:fake")).toBe("fake");
    expect(markToken("analysed:no")).toBe("unanalysed");
    expect(markToken("marked:")).toBeUndefined();
    expect(markToken("constructor")).toBeUndefined();
  });
});

describe("a chip's hint", () => {
  it("names the judgement with its note, or else the analyser's reading", () => {
    expect(markHint(fake)).toBe("You marked this fake: an old rip");
    expect(markHint(reading)).toBe("The analyser's reading: likely a transcode, mp3 ~128 kbps");
  });
});
