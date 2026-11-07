import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";

import { type Interests, OwnInterests } from "./interests.svelte";
import { ownProfile } from "./profile.svelte";

/** Our profile holding `interests` */
const profile = (interests: Interests) => ({
  description: "",
  hasPicture: false,
  pictureUrl: null,
  uploadSlots: 1,
  freeUploadSlot: true,
  queueLength: 0,
  interests,
  sources: {
    description: { layer: "default" },
    picture: { layer: "default" },
    liked: { layer: "file", file: "config.toml" },
    hated: { layer: "file", file: "config.toml" }
  }
});

describe("our own interests", () => {
  afterEach(() => void vi.unstubAllGlobals());

  it("show what the profile holds once the saves are done, whichever of them failed", async () => {
    let held: Interests = { liked: [], hated: [] };
    let puts = 0;
    fakeNetwork((_url, method, body) => {
      if (method === "GET") return profile(held);
      if (++puts === 1) return { status: 500, body: { error: "busy" } };
      held = body as Interests;
      return { status: 204 };
    });
    const interests = new OwnInterests();
    await ownProfile.loader.load();
    interests.add("liked", "jazz");
    interests.add("liked", "folk");
    await interests.settled;
    expect(interests.lists.liked).toEqual(["jazz", "folk"]);

    // what another tab saves shows on the next read
    held = { liked: ["jazz"], hated: ["folk"] };
    await ownProfile.loader.load();
    expect(interests.lists).toEqual(held);
  });

  it("skip one already listed and move one from the other list, whatever its case", async () => {
    let held: Interests = { liked: ["jazz"], hated: [] };
    const puts: Interests[] = [];
    fakeNetwork((_url, method, body) => {
      if (method === "GET") return profile(held);
      held = body as Interests;
      puts.push(held);
      return { status: 204 };
    });
    const interests = new OwnInterests();
    await ownProfile.loader.load();
    interests.add("liked", " Jazz ");
    await interests.settled;
    expect(puts).toEqual([]);

    interests.add("hated", "JAZZ");
    await interests.settled;
    expect(puts).toEqual([{ liked: [], hated: ["jazz"] }]);
  });
});
