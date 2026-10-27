import { describe, expect, it } from "vitest";

import { linkify, snippetParts } from "./text";

describe("links in text", () => {
  it("makes http and https runs links and keeps the rest as text", () => {
    expect(linkify("see https://a.example/x?y=1, or http://b.example.")).toEqual([
      { at: 0, text: "see " },
      { at: 4, text: "https://a.example/x?y=1", url: "https://a.example/x?y=1" },
      { at: 27, text: ", or " },
      { at: 32, text: "http://b.example", url: "http://b.example" },
      { at: 48, text: "." }
    ]);
  });

  it("leaves other schemes, bare hosts and markup as text", () => {
    for (const text of ["javascript:alert(1)", "www.example.com", "<a href=x>y</a>", ""]) {
      expect(linkify(text).every((p) => !p.url)).toBe(true);
    }
  });

  it("ends a link at a space or a quote, and keeps a closing bracket out", () => {
    expect(linkify('(https://x.example/a) "https://y.example"').map((p) => p.text)).toEqual([
      "(",
      "https://x.example/a",
      ') "',
      "https://y.example",
      '"'
    ]);
  });
});

describe("search snippets", () => {
  it("reads the marked matches and unescapes the rest as text", () => {
    expect(
      snippetParts("a &lt;b&gt; <mark>pink</mark> &amp; <mark>floyd</mark>&quot;").map((p) => [
        p.text,
        p.hit
      ])
    ).toEqual([
      ["a <b> ", false],
      ["pink", true],
      [" & ", false],
      ["floyd", true],
      ['"', false]
    ]);
  });
});
