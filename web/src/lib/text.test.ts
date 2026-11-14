import { describe, expect, it } from "vitest";

import { fold, linkify, pathParts, snippetParts, splitTail } from "./text";

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

describe("cutting in the middle", () => {
  it("keeps a path's last part whole", () => {
    expect(splitTail("@@music\\Artist\\2000 - Album\\CD1", true)).toEqual([
      "@@music\\Artist\\2000 - Album",
      "\\CD1"
    ]);
    expect(splitTail("/home/ryu/Music", true)).toEqual(["/home/ryu", "/Music"]);
  });

  it("keeps the end of a text with no separator, or with a last part too long to keep", () => {
    expect(splitTail("01 - Storm - Lift Yr. Skinny Fists.flac")).toEqual([
      "01 - Storm - Lift Yr. Skinn",
      "y Fists.flac"
    ]);
    expect(splitTail("x\\" + "a".repeat(60), true)[1]).toHaveLength(12);
    expect(splitTail("short")).toEqual(["", "short"]);
  });

  it("keeps as wide an end in a script whose letters are twice as wide", () => {
    expect(splitTail("1_戦場のメリークリスマス.flac")).toEqual([
      "1_戦場のメリークリ",
      "スマス.flac"
    ]);
    // a last part as wide as the most kept in Latin letters is too wide in these
    const deep = "@@a\\音楽\\" + "坂".repeat(30);
    expect(splitTail(deep, true)).toEqual([deep.slice(0, -6), "坂".repeat(6)]);
  });

  it("never cuts inside a letter drawn from several code points", () => {
    for (const name of ["🔥".repeat(9) + ".flac", "👩🏽‍💻".repeat(4) + "-x.mp3", "Tiến.flac"]) {
      const [head, tail] = splitTail("x".repeat(20) + name);
      const whole = new Intl.Segmenter().segment(head + tail);
      const at = [...whole].map((s) => s.index);
      expect(at, name).toContain(head.length);
    }
  });

  it("splits a path into its parts and separators", () => {
    expect(pathParts("@@abd\\موسيقى\\أم كلثوم")).toEqual([
      "@@abd",
      "\\",
      "موسيقى",
      "\\",
      "أم كلثوم"
    ]);
    expect(pathParts("/home/ryu")).toEqual(["/", "home", "/", "ryu"]);
  });
});

describe("folded text", () => {
  it("keeps the start up to so many characters or lines, and nothing when it all fits", () => {
    expect(fold("x".repeat(1_000_000), 1000)).toBe("x".repeat(1000));
    expect(fold("a\nb\nc\nd", 1000, 2)).toBe("a\nb");
    expect(fold("a\nb", 1000, 2)).toBeNull();
    expect(fold("short", 1000)).toBeNull();
  });

  it("never cuts inside a letter", () => {
    expect(fold("a" + "🔥".repeat(600), 1000)).toBe("a" + "🔥".repeat(499));
    expect(fold("x" + "👩🏽‍💻".repeat(200), 1000)).toBe("x" + "👩🏽‍💻".repeat(142));
  });
});
