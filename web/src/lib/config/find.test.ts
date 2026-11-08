import { describe, expect, it } from "vitest";

import type { FieldView } from "./fields";
import { currentSection, fieldMatches } from "./find";

const info = (fields: Partial<FieldView>) => fields as FieldView;

describe("the settings search", () => {
  const field = { key: "shares.share_downloads", label: "Share downloads folder" };
  const view = info({
    doc: "Adds the downloads directory to the shares.",
    envVar: "RESEEK_SHARES_SHARE_DOWNLOADS",
    cliFlag: "--shares-share-downloads"
  });

  it("finds an option by any of its words, in any order and case", () => {
    expect(fieldMatches(field, view, "")).toBe(true);
    expect(fieldMatches(field, view, "Downloads SHARE")).toBe(true);
    expect(fieldMatches(field, view, "share_downloads")).toBe(true);
    expect(fieldMatches(field, view, "shares.share_downloads")).toBe(true);
    expect(fieldMatches(field, view, "directory")).toBe(true);
    expect(fieldMatches(field, view, "RESEEK_SHARES")).toBe(true);
    expect(fieldMatches(field, view, "--shares-share")).toBe(true);
  });

  it("needs every word", () => {
    expect(fieldMatches(field, view, "share uploads")).toBe(false);
    expect(fieldMatches(field, undefined, "directory")).toBe(false);
  });
});

describe("the section in view", () => {
  const tops = [
    { id: "soulseek", top: -900 },
    { id: "shares", top: -100 },
    { id: "transfers", top: 300 }
  ];

  it("is the last section whose top passed the line under the header", () => {
    expect(currentSection(tops, 80)).toBe("shares");
    expect(currentSection(tops, 300)).toBe("transfers");
    expect(
      currentSection(
        [
          { id: "a", top: 200 },
          { id: "b", top: 900 }
        ],
        80
      )
    ).toBe("a");
    expect(currentSection([], 80)).toBeNull();
  });

  it("is the last section at the end of the page", () => {
    expect(currentSection(tops, 80, true)).toBe("transfers");
  });
});
