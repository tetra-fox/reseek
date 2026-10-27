import fc from "fast-check";
import { describe, expect, it } from "vitest";

import { type FilterFile, fileMatches, filterResponse, parseFilters } from "./filters";

const file = (fields: Partial<FilterFile>): FilterFile => ({
  filename: "a\\b\\c.mp3",
  size: 1000,
  ...fields
});

describe("parseFilters", () => {
  it.each([
    ["foo minbr:42 bar", { minBitRate: 42, include: ["foo", "bar"] }],
    ["foo minbitrate:123 bar", { minBitRate: 123 }],
    ["foo minfs:42 bar", { minFileSize: 42 }],
    ["foo minfilesize:123 bar", { minFileSize: 123 }],
    ["foo minlen:42 bar", { minLength: 42 }],
    ["foo minlength:123 bar", { minLength: 123 }],
    ["foo minfif:42 bar", { minFilesInFolder: 42 }],
    ["foo minfilesinfolder:123 bar", { minFilesInFolder: 123 }],
    ["foo minbd:24 bar", { minBitDepth: 24 }],
    ["foo iscbr isvbr bar", { include: ["foo", "bar"], isCBR: true, isVBR: true }],
    ["foo some:thing bar", { include: ["foo", "bar"] }],
    ["foo -bar", { include: ["foo"], exclude: ["bar"] }],
    ["-foo -bar -baz qux", { exclude: ["foo", "bar", "baz"], include: ["qux"] }],
    [
      "big -mix of:everything isvbr iscbr minbr:42",
      { exclude: ["mix"], include: ["big"], isCBR: true, isVBR: true, minBitRate: 42 }
    ]
  ])("reads %j", (text, expected) => {
    expect(parseFilters(text)).toMatchObject(expected);
  });

  it("splits on any whitespace and drops empty terms and a lone dash", () => {
    expect(parseFilters("  foo \t bar\n - ")).toMatchObject({
      include: ["foo", "bar"],
      exclude: []
    });
  });

  it("takes flags and minimums only as whole tokens", () => {
    const filters = parseFilters("-isvbr isvbrx minbr:1x MINBR:5 minbr:9");
    expect(filters).toMatchObject({
      isVBR: false,
      exclude: ["isvbr"],
      include: ["isvbrx"],
      minBitRate: 5
    });
  });

  it("never throws, whatever is typed", () => {
    fc.assert(
      fc.property(fc.string({ unit: "binary" }), (text) => {
        const filters = parseFilters(text);
        expect(filters.include.every(Boolean)).toBe(true);
        expect(filters.exclude.every(Boolean)).toBe(true);
      })
    );
  });
});

describe("fileMatches", () => {
  const matches = (text: string, fields: Partial<FilterFile>) =>
    fileMatches(parseFilters(text), file(fields));

  it("keeps constant or variable bit rates", () => {
    expect(matches("iscbr", { bitRate: 320, isVariableBitRate: false })).toBe(true);
    expect(matches("iscbr", { bitRate: 123, isVariableBitRate: true })).toBe(false);
    expect(matches("iscbr", { bitRate: 320 })).toBe(false);
    expect(matches("isvbr", { bitRate: 123, isVariableBitRate: true })).toBe(true);
    expect(matches("isvbr", { bitRate: 320, isVariableBitRate: false })).toBe(false);
    expect(matches("iscbr isvbr", { isVariableBitRate: true })).toBe(false);
    expect(matches("iscbr isvbr", { isVariableBitRate: false })).toBe(false);
  });

  it("keeps lossless or lossy files", () => {
    expect(matches("islossless", { bitDepth: 16, sampleRate: 41000 })).toBe(true);
    expect(matches("islossless", { bitRate: 320, isVariableBitRate: false })).toBe(false);
    expect(matches("islossy", { bitRate: 320, isVariableBitRate: false })).toBe(true);
    expect(matches("islossy", { bitDepth: 16, sampleRate: 41000 })).toBe(false);
  });

  it("drops files below a minimum and keeps those without the attribute", () => {
    expect(matches("minbr:100", { bitRate: 100 })).toBe(true);
    expect(matches("minbr:100", { bitRate: 99 })).toBe(false);
    expect(matches("minbr:100", { bitRate: null })).toBe(true);
    expect(matches("minfs:100", { size: 100 })).toBe(true);
    expect(matches("minfs:100", { size: 99 })).toBe(false);
    expect(matches("minlen:100", { length: 100 })).toBe(true);
    expect(matches("minlen:100", { length: 99 })).toBe(false);
    expect(matches("minbd:24", { bitDepth: 16 })).toBe(false);
  });

  it("matches words anywhere in the lower-cased path", () => {
    expect(matches("path to .nfo", { filename: "/path/to/info.nfo" })).toBe(true);
    expect(matches("-jpg", { filename: "Album\\Folder.JPG" })).toBe(false);
    expect(matches("ALBUM", { filename: "x\\album\\1.flac" })).toBe(true);
  });
});

describe("filterResponse", () => {
  const response = {
    username: "u",
    files: [file({ filename: "a.mp3" }), file({ filename: "b.flac" })],
    lockedFiles: [file({ filename: "c.mp3" })],
    fileCount: 2,
    lockedFileCount: 1
  };

  it("filters both lists and counts what is left", () => {
    expect(filterResponse(parseFilters("mp3"), response)).toMatchObject({
      fileCount: 1,
      lockedFileCount: 1,
      files: [{ filename: "a.mp3" }]
    });
  });

  it("empties a response with fewer files than minfif, locked ones counted", () => {
    expect(filterResponse(parseFilters("minfif:3"), response).fileCount).toBe(2);
    expect(filterResponse(parseFilters("minfif:4"), response)).toMatchObject({
      files: [],
      lockedFiles: [],
      fileCount: 0,
      lockedFileCount: 0
    });
  });
});
