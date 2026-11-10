import { afterEach, describe, expect, it, vi } from "vitest";

import { toasts } from "#lib/feedback/toast.svelte.js";
import { changed } from "#lib/testing/fixtures.js";
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
  },
  optionsRevision: "read"
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
      return changed();
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
      return changed();
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

  it("are each saved at the revision the save before left, and refused after a change elsewhere", async () => {
    // a daemon that refuses a change made against options it has changed since
    let held: Interests = { liked: [], hated: [] };
    let revision = "read";
    let changes = 0;
    const net = fakeNetwork((url, method, body) => {
      if (method === "GET") return { ...profile(held), optionsRevision: revision };
      if (url.searchParams.get("revision") !== revision) {
        return {
          status: 409,
          body: { error: "The options changed since they were read", detail: revision }
        };
      }
      held = body as Interests;
      revision = `changed ${++changes}`;
      return changed(revision);
    });
    const interests = new OwnInterests();
    await ownProfile.loader.load();
    interests.add("liked", "jazz");
    interests.add("liked", "folk");
    await interests.settled;
    expect(held.liked).toEqual(["jazz", "folk"]);

    // another admin's change lands before this tab's next one
    held = { liked: ["jazz", "folk", "dub"], hated: [] };
    revision = "elsewhere";
    interests.add("hated", "polka");
    await interests.settled;
    expect(toasts.map((t) => t.text)).toContain(
      "Interests changed meanwhile. Check your changes and save again."
    );
    expect(held.hated, "the other admin's list stays").toEqual([]);
    expect(interests.lists, "the daemon's lists show").toEqual(held);
    expect(net.requests.filter((r) => r.method === "PUT")).toHaveLength(3);
    toasts.splice(0);
  });
});
