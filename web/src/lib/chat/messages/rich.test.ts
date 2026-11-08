import { describe, expect, it } from "vitest";

import { actionOf, richText, slskText } from "./rich";

const kinds = (text: string, watch: string[] = []) =>
  richText(text, watch).map((p) => [p.kind, p.text]);

describe("a chat message's text", () => {
  it("makes web and slsk links of what is one, trailing punctuation left out", () => {
    expect(kinds("see https://example.com/a?b=1, or slsk://bob/Music/Album/.")).toEqual([
      ["text", "see "],
      ["url", "https://example.com/a?b=1"],
      ["text", ", or "],
      ["slsk", "slsk://bob/Music/Album/"],
      ["text", "."]
    ]);
    const [, link] = richText("got slsk://al%20ice/a/b.flac");
    expect(link.kind === "slsk" && link.link).toEqual({
      username: "al ice",
      path: "a\\b.flac",
      folder: false
    });
    // shown decoded, as typed before it was encoded
    expect(link.kind === "slsk" && slskText(link.link)).toBe("slsk://al ice/a/b.flac");
    const [folder] = richText("slsk://bob/My%20Music/%D9%85%D9%88%D8%B3%D9%8A%D9%82%D9%89/");
    expect(folder.kind === "slsk" && slskText(folder.link)).toBe("slsk://bob/My Music/موسيقى/");
    // not a link to anything: plain text
    expect(kinds("slsk://nobody")).toEqual([["text", "slsk://nobody"]]);
  });

  it("marks watched words only as whole words, case folded, a phrase before its first word", () => {
    expect(kinds("Pink Floyd. floyds? FLOYD_x pink floyd", ["floyd", "pink floyd"])).toEqual([
      ["mark", "Pink Floyd"],
      ["text", ". floyds? FLOYD_x "],
      ["mark", "pink floyd"]
    ]);
    expect(kinds("hey alice-bob", ["alice"])).toEqual([
      ["text", "hey "],
      ["mark", "alice"],
      ["text", "-bob"]
    ]);
    expect(kinds("a.b (c) [d]", ["a.b", "(c)", " ", ""])).toEqual([
      ["mark", "a.b"],
      ["text", " "],
      ["mark", "(c)"],
      ["text", " [d]"]
    ]);
  });

  it("keeps every character, in order", () => {
    const text = "x https://a.b/c alice slsk://u/p y";
    expect(
      richText(text, ["alice"])
        .map((p) => p.text)
        .join("")
    ).toBe(text);
  });

  it("reads a /me message as an action", () => {
    expect(actionOf("/me waves")).toBe("waves");
    expect(actionOf("/me")).toBeNull();
    expect(actionOf("/meow")).toBeNull();
    expect(actionOf("hi /me waves")).toBeNull();
  });
});
