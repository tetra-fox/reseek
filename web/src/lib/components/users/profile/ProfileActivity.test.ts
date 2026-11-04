import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import ProfileActivity from "./ProfileActivity.svelte";

describe("a user's activity", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("shows the most asked for when Load more is pressed twice", async () => {
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    fakeNetwork(async (url) => {
      if (url.pathname.endsWith("/events")) return [];
      if (!url.pathname.endsWith("/transfers")) return undefined;
      const limit = Number(url.searchParams.get("limit"));
      if (limit === 100) await held;
      const transfers = Array.from({ length: limit }, (_, n) => ({
        id: `t${n}`,
        direction: "Download",
        filename: `@@a\\${n}.flac`,
        state: "Queued, Remotely",
        requestedAt: new Date(Date.UTC(2026, 9, 1, 0, n)).toISOString()
      }));
      return { transfers, total: 1000 };
    });
    const component = mount(ProfileActivity, {
      target: document.body,
      props: { username: "alice" }
    });
    await flush();
    const more = () =>
      [...document.querySelectorAll("button")].find((b) => b.textContent?.trim() === "Load more")!;
    more().click();
    await flush();
    more().click();
    await flush();
    release();
    await flush();
    expect(document.querySelectorAll("li")).toHaveLength(150);
    unmount(component);
  });
});
