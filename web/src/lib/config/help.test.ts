import { describe, expect, it } from "vitest";

import { helpParts } from "./help";

describe("option help", () => {
  it("shows what is between backticks as code", () => {
    expect(helpParts("Default frequency scale: `log`, `linear` or `mel`.")).toEqual([
      { text: "Default frequency scale: ", code: false },
      { text: "log", code: true },
      { text: ", ", code: false },
      { text: "linear", code: true },
      { text: " or ", code: false },
      { text: "mel", code: true },
      { text: ".", code: false }
    ]);
    expect(helpParts("`0` turns it off")).toEqual([
      { text: "0", code: true },
      { text: " turns it off", code: false }
    ]);
  });

  it("keeps text without a pair of backticks as it is", () => {
    expect(helpParts("Plain help.")).toEqual([{ text: "Plain help.", code: false }]);
    expect(helpParts("A lone ` stays")).toEqual([{ text: "A lone ` stays", code: false }]);
    expect(helpParts("`a` and ` b")).toEqual([
      { text: "a", code: true },
      { text: " and ` b", code: false }
    ]);
    expect(helpParts("Empty `` pair")).toEqual([
      { text: "Empty ", code: false },
      { text: " pair", code: false }
    ]);
    expect(helpParts("")).toEqual([]);
  });
});
