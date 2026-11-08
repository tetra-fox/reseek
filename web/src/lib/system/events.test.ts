import { describe, expect, it } from "vitest";

import { payloadText } from "./events";

describe("payloadText", () => {
  it("writes data without line breaks as JSON.stringify does", () => {
    const data = {
      version: 0,
      name: 'a "quoted" \\ name',
      scopes: ["uploads", "rooms"],
      empty: [],
      none: null,
      nested: { ok: true, inner: {} }
    };
    expect(payloadText(data)).toBe(JSON.stringify(data, null, 2));
  });

  it("writes a string of several lines on its lines, each escaped", () => {
    const data = { output: 'one\ntwo "2"\n\tthree', list: ["a\nb"] };
    expect(payloadText(data)).toBe(
      [
        "{",
        '  "output": "one',
        '    two \\"2\\"',
        '    \\tthree",',
        '  "list": [',
        '    "a',
        '      b"',
        "  ]",
        "}"
      ].join("\n")
    );
  });
});
