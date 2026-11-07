import { describe, expect, it } from "vitest";

import { entries } from "./explorer";

const listing = {
  name: "",
  fullName: "",
  attributes: "Directory",
  createdAt: "2026-10-26T10:00:00Z",
  modifiedAt: "2026-10-26T10:00:00Z",
  directories: [
    { name: "b10", fullName: "b10", modifiedAt: "2026-10-26T09:00:00Z" },
    { name: "b9", fullName: "b9", modifiedAt: "2026-10-26T11:00:00Z" }
  ],
  files: [
    { name: "z.flac", fullName: "z.flac", length: 5, modifiedAt: "2026-10-26T08:00:00Z" },
    { name: "A.flac", fullName: "A.flac", length: 7, modifiedAt: "2026-10-26T12:00:00Z" }
  ]
} as never;

describe("the file explorer's lines", () => {
  it("puts the way up first, then directories, then files, by name with numbers in order", () => {
    const lines = entries(listing, ["x", "y"], "name", "asc");
    expect(lines.map((l) => l.name)).toEqual(["..", "b9", "b10", "A.flac", "z.flac"]);
    expect(lines[0].path).toBe("x");
    expect(lines[1].path).toBe("x/y/b9");
  });

  it("sorts by date each way and keeps the way up out of a root listing", () => {
    expect(entries(listing, [], "date", "desc").map((l) => l.name)).toEqual([
      "b9",
      "b10",
      "A.flac",
      "z.flac"
    ]);
    expect(entries(listing, [], "date", "asc").map((l) => l.name)).toEqual([
      "b10",
      "b9",
      "z.flac",
      "A.flac"
    ]);
  });
});
