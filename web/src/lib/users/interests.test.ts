import { afterEach, describe, expect, it, vi } from "vitest";

import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { type Interests, OwnInterests } from "./interests.svelte";

describe("our own interests", () => {
  afterEach(() => void vi.unstubAllGlobals());

  it("show what the daemon holds once the saves are done, whichever of them failed", async () => {
    let held: Interests = { liked: [], hated: [] };
    let puts = 0;
    fakeNetwork((_url, method, body) => {
      if (method === "GET") return held;
      if (++puts === 1) return { status: 500, body: { error: "busy" } };
      held = body as Interests;
      return held;
    });
    const interests = new OwnInterests();
    await interests.loader.load();
    interests.add("liked", "jazz");
    interests.add("liked", "folk");
    await flush();
    expect(interests.lists.liked).toEqual(["jazz", "folk"]);

    // what another tab saves shows on the next read
    held = { liked: ["jazz"], hated: ["folk"] };
    await interests.loader.load();
    expect(interests.lists).toEqual(held);
  });

  it("skip one already listed and move one from the other list, whatever its case", async () => {
    let held: Interests = { liked: ["jazz"], hated: [] };
    const puts: Interests[] = [];
    fakeNetwork((_url, method, body) => {
      if (method === "GET") return held;
      held = body as Interests;
      puts.push(held);
      return held;
    });
    const interests = new OwnInterests();
    await interests.loader.load();
    interests.add("liked", " Jazz ");
    await flush();
    expect(puts).toEqual([]);

    interests.add("hated", "JAZZ");
    await flush();
    expect(puts).toEqual([{ liked: [], hated: ["jazz"] }]);
  });
});
