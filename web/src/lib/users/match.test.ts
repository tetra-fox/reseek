import { describe, expect, it } from "vitest";

import { bestMatches, nameParts } from "./match.js";
const ranked = (names: string[], text: string) => bestMatches(names, text, (name) => name);

describe("bestMatches", () => {
  it("puts the exact name first, then the name in another case", () => {
    expect(ranked(["bobby", "BOB", "bob"], "bob")).toEqual(["bob", "BOB", "bobby"]);
    expect(ranked(["bob", "BOB"], "BOB")).toEqual(["BOB", "bob"]);
  });

  it("orders a start of the name, a start of a word, then anywhere", () => {
    expect(ranked(["jacobs", "dj_cob", "cobalt"], "cob")).toEqual(["cobalt", "dj_cob", "jacobs"]);
  });

  it("finds a word start past an earlier match inside a word", () => {
    expect(ranked(["acob_cobalt", "jacob"], "cob")).toEqual(["acob_cobalt", "jacob"]);
  });

  it("keeps shorter names first, then the order the users came in", () => {
    expect(ranked(["alice-long", "alice2", "alice1"], "ali")).toEqual([
      "alice2",
      "alice1",
      "alice-long"
    ]);
  });

  it("leaves out an item whose name does not hold the text", () => {
    expect(ranked(["alice", "tagged"], "jazz")).toEqual([]);
  });

  it("takes the text literally", () => {
    expect(ranked(["a.b", "axb"], "a.b")).toEqual(["a.b"]);
  });
});

describe("nameParts", () => {
  it("marks where a name holds the text, in any case", () => {
    expect(nameParts("DJ Cobalt", "cob")).toEqual([
      { at: 0, text: "DJ ", hit: false },
      { at: 3, text: "Cob", hit: true },
      { at: 6, text: "alt", hit: false }
    ]);
    expect(nameParts("alice", "bob")).toEqual([{ at: 0, text: "alice", hit: false }]);
  });
});
