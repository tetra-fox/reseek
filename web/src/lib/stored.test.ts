import { flushSync } from "svelte";
import { describe, expect, it } from "vitest";

import { fields, stored } from "./stored.svelte";

const strings = (list: unknown) =>
  Array.isArray(list) ? list.filter((s): s is string => typeof s === "string") : [];

/** Writes `value` under `key` as another tab of this browser does, which this tab hears of */
const otherTab = (key: string, value: unknown) => {
  const json = JSON.stringify(value);
  localStorage.setItem(key, json);
  window.dispatchEvent(new StorageEvent("storage", { key, newValue: json }));
};

describe("a stored value", () => {
  it("takes another tab's write, so its own next write keeps what that tab did", () => {
    localStorage.setItem("test.recent", JSON.stringify({ texts: ["a"], pinned: [] }));
    const recent = stored("test.recent", (saved) => ({
      texts: strings(fields(saved).texts),
      pinned: strings(fields(saved).pinned)
    }));
    otherTab("test.recent", { texts: ["a"], pinned: ["a"] });
    expect(recent.pinned).toEqual(["a"]);
    recent.texts = ["b", "a"];
    flushSync();
    expect(JSON.parse(localStorage.getItem("test.recent")!)).toEqual({
      texts: ["b", "a"],
      pinned: ["a"]
    });
  });

  it("drops what another tab took out of a value kept by name", () => {
    const drafts = stored("test.drafts", (saved) =>
      Object.fromEntries(
        Object.entries(fields(saved)).filter((e): e is [string, string] => typeof e[1] === "string")
      )
    );
    drafts.one = "hello";
    drafts.two = "there";
    flushSync();
    otherTab("test.drafts", { two: "there" });
    expect({ ...drafts }).toEqual({ two: "there" });
    otherTab("test.other", { one: "again" });
    expect({ ...drafts }).toEqual({ two: "there" });
  });
});
