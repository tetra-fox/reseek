import fc from "fast-check";
import { describe, expect, it } from "vitest";

import { type Field, type FilterFile, fileMatches, filterResponse, parseFilters } from "./filters";

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
    const filters = parseFilters("-isvbr isvbrx minbr:1x MINBR:5 minbr:9 constructor __proto__");
    expect(filters).toMatchObject({
      isVBR: false,
      exclude: ["isvbr"],
      include: ["isvbrx", "constructor", "__proto__"],
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

describe("field tokens", () => {
  const matches = (text: string, fields: Partial<FilterFile>) =>
    fileMatches(parseFilters(text), file(fields));
  const MB = 1024 ** 2;

  it.each([
    ["size:10m", { conditions: [{ field: "size", alternatives: [{ op: ">=", value: 10 * MB }] }] }],
    ["size:<1.5g", { conditions: [{ alternatives: [{ op: "<", value: 1.5 * 1024 ** 3 }] }] }],
    [
      "br:=256|=320",
      { conditions: [{ field: "bitrate", alternatives: [{ value: 256 }, { value: 320 }] }] }
    ],
    ["bitrate:!128", { conditions: [{ alternatives: [{ op: "!=", value: 128 }] }] }],
    [
      "length:3:00..6:00",
      { conditions: [{ field: "length", alternatives: [{ from: 180, to: 360 }] }] }
    ],
    ["len:1:02:03", { conditions: [{ alternatives: [{ op: ">=", value: 3723 }] }] }],
    ["!bitrate:320", { conditions: [{ negated: true }] }],
    ["type:FLAC|.mp3", { types: [{ values: ["flac", "mp3"], negated: false }] }],
    ["!type:mp3 -ext:ogg", { types: [{ negated: true }, { values: ["ogg"], negated: true }] }],
    ["country:NL|be", { countries: [{ values: ["nl", "be"], negated: false }] }],
    ["size:lots bitrate: type: foo", { conditions: [], types: [], include: ["foo"] }],
    ["size:5m..", { conditions: [{ alternatives: [{ from: 5 * MB, to: Infinity }] }] }]
  ])("reads %j", (text, expected) => {
    expect(parseFilters(text)).toMatchObject(expected);
  });

  it("compares sizes, bit rates and lengths", () => {
    expect(matches("size:1k..2k", { size: 1500 })).toBe(true);
    expect(matches("size:1k..2k", { size: 2049 })).toBe(false);
    expect(matches("size:>1000", { size: 1000 })).toBe(false);
    expect(matches("size:<=1000", { size: 1000 })).toBe(true);
    expect(matches("br:=256|=320", { bitRate: 320 })).toBe(true);
    expect(matches("br:=256|=320", { bitRate: 192 })).toBe(false);
    expect(matches("br:!128", { bitRate: 128 })).toBe(false);
    expect(matches("!br:128", { bitRate: 200 })).toBe(false);
    expect(matches("!br:128", { bitRate: 96 })).toBe(true);
    expect(matches("length:<5:00", { length: 299 })).toBe(true);
    expect(matches("length:<5:00", { length: 300 })).toBe(false);
    expect(matches("bd:24", { bitDepth: 16 })).toBe(false);
  });

  it("leaves files without the attribute in", () => {
    expect(matches("bitrate:320 length:5:00", { bitRate: null })).toBe(true);
  });

  it("keeps files of a type, by extension or by name", () => {
    expect(matches("type:flac|mp3", { filename: "a\\b.MP3" })).toBe(true);
    expect(matches("type:flac", { filename: "a.mp3", extension: "flac" })).toBe(true);
    expect(matches("type:flac", { filename: "noextension" })).toBe(false);
    expect(matches("!type:mp3", { filename: "a.mp3" })).toBe(false);
    expect(matches("!type:mp3", { filename: "a.flac" })).toBe(true);
  });

  /** what a comparison of `value` with `typed` should give, worked out apart from the parser */
  const expected = (op: string, typed: number, value: number) =>
    ({ ">=": value >= typed, ">": value > typed, "<=": value <= typed, "<": value < typed })[op];

  it("agrees with the comparison it was written as", () => {
    const fields: [Field, keyof FilterFile][] = [
      ["size", "size"],
      ["bitrate", "bitRate"],
      ["bitdepth", "bitDepth"],
      ["length", "length"]
    ];
    fc.assert(
      fc.property(
        fc.constantFrom(...fields),
        fc.constantFrom(">=", ">", "<=", "<", ""),
        fc.nat(100_000),
        fc.nat(100_000),
        ([field, attribute], op, typed, value) => {
          const filters = parseFilters(`${field}:${op}${typed}`);
          const kept = fileMatches(filters, file({ [attribute]: value }));
          expect(kept).toBe(expected(op || ">=", typed, value));
        }
      )
    );
  });

  it("only narrows what passes as tokens are added", () => {
    fc.assert(
      fc.property(
        fc.string(),
        fc.string(),
        fc.record({
          filename: fc.string(),
          size: fc.nat(),
          bitRate: fc.option(fc.nat(2000)),
          length: fc.option(fc.nat(10_000)),
          isVariableBitRate: fc.option(fc.boolean())
        }),
        (text, more, shape) => {
          const subject = shape as FilterFile;
          // a new token passes or fails on its own, so what it keeps was kept before it
          if (fileMatches(parseFilters(`${text} ${more}`), subject)) {
            expect(fileMatches(parseFilters(text), subject)).toBe(true);
          }
        }
      )
    );
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

  it("keeps a response by its user's country", () => {
    const from = (country: string | null) => ({ ...response, country });
    expect(filterResponse(parseFilters("country:nl|be"), from("NL")).fileCount).toBe(2);
    expect(filterResponse(parseFilters("country:nl"), from("DE")).fileCount).toBe(0);
    expect(filterResponse(parseFilters("country:nl"), from(null)).fileCount).toBe(0);
    expect(filterResponse(parseFilters("!country:nl"), from(null)).fileCount).toBe(2);
    expect(filterResponse(parseFilters("!country:nl"), from("NL")).fileCount).toBe(0);
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
