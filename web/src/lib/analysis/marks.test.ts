import { describe, expect, it } from "vitest";

import { type Mark, markHint, markToken, passes } from "./marks";

const reading: Mark = {
  verdict: "likely_transcode",
  profile: "mp3 ~128 kbps",
  judgment: null,
  note: null,
  stale: false
};
const fake: Mark = { ...reading, judgment: "fake", note: "an old rip" };

describe("filtering files by their analysis", () => {
  it("tells marked, analyzed and not analyzed files apart", () => {
    expect(passes("fake", fake, true)).toBe(true);
    expect(passes("fake", reading, true)).toBe(false);
    expect(passes("genuine", { ...reading, judgment: "legit" }, true)).toBe(true);
    expect(passes("analyzed", reading, true)).toBe(true);
    expect(passes("analyzed", undefined, true)).toBe(false);
    expect(passes("unanalyzed", undefined, true)).toBe(true);
    // a cover image is not waiting to be analyzed
    expect(passes("unanalyzed", undefined, false)).toBe(false);
  });

  it("goes by the analyzer's reading even when the user judged the file", () => {
    expect(passes("reading:likely_transcode", fake, true)).toBe(true);
    expect(passes("reading:consistent", fake, true)).toBe(false);
    expect(passes("reading:consistent", undefined, true)).toBe(false);
  });

  it("reads the filter language's analysis words and no others", () => {
    expect(markToken("marked:fake")).toBe("fake");
    expect(markToken("analyzed:no")).toBe("unanalyzed");
    expect(markToken("marked:")).toBeUndefined();
    expect(markToken("constructor")).toBeUndefined();
  });
});

describe("a chip's hint", () => {
  it("names the judgment with its note, or else the analyzer's reading", () => {
    expect(markHint(fake)).toBe("You marked this fake: an old rip");
    expect(markHint(reading)).toBe("The analyzer's reading: likely a transcode, mp3 ~128 kbps");
    expect(markHint({ ...reading, stale: true })).toBe("Analyzed by an older version of reseek");
  });
});
