import { describe, expect, it } from "vitest";

import { typeAhead } from "./typeahead";

describe("type-ahead in the folder tree", () => {
  const names = ["Albums", "Artists", "Bootlegs", "artwork", "Compilations"];

  it("finds the next folder starting with what was typed, wrapping", () => {
    expect(typeAhead(names, -1, "b")).toBe(2);
    expect(typeAhead(names, 2, "c")).toBe(4);
    expect(typeAhead(names, 4, "a")).toBe(0);
    expect(typeAhead(names, 0, "z")).toBe(-1);
  });

  it("keeps the match while a word grows, and steps on with a repeated letter", () => {
    expect(typeAhead(names, 0, "al")).toBe(0);
    expect(typeAhead(names, 0, "ar")).toBe(1);
    expect(typeAhead(names, 1, "art")).toBe(1);
    expect(typeAhead(names, 1, "artw")).toBe(3);
    expect(typeAhead(names, 0, "aa")).toBe(1);
    expect(typeAhead(names, 1, "aaa")).toBe(3);
  });
});
