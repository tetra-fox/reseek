import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { RoomMembers } from "./room.svelte";

const jazz = (ticker: string): Schemas["Room"] => ({
  name: "jazz",
  isPrivate: false,
  tickers: [{ username: "bob", ticker }],
  users: []
});

describe("a room's members", () => {
  afterEach(() => {
    live.stop();
    vi.unstubAllGlobals();
  });

  it("keep the room a push brought while a read from before it was on its way", async () => {
    let answer: ((room: unknown) => void) | undefined;
    let reads = 0;
    const net = fakeNetwork((url) => {
      if (!url.pathname.endsWith("/rooms/joined/jazz")) return undefined;
      reads += 1;
      if (reads === 1) return { status: 503, body: { error: "the daemon is restarting" } };
      return new Promise((resolve) => (answer = resolve));
    });
    live.start();
    net.frame("hello", { principal: { name: "me", role: "Administrator" } } as never);
    const members = new RoomMembers("jazz");
    const stop = $effect.root(() => members.follow());
    flushSync();

    await members.load();
    expect(members.error).toBe("the daemon is restarting");

    // Retry
    const reading = members.load();
    await vi.waitFor(() => expect(answer).toBeDefined());
    net.frame("rooms.update", jazz("new"));
    await vi.waitFor(() => expect(members.wall.map((l) => l.ticker)).toEqual(["new"]));
    // read before bob changed his ticker
    answer!(jazz("old"));
    await reading;

    expect(members.wall.map((l) => l.ticker)).toEqual(["new"]);
    expect(members.room?.tickers[0].ticker).toBe("new");
    expect(members.error).toBe("");
    stop();
  });
});
