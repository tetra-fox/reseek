import { afterEach, describe, expect, it, vi } from "vitest";

import { chat } from "#lib/chat/chat.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { ChatSearch } from "./search.svelte";

const hit = {
  kind: "private",
  room: null,
  username: "alice",
  timestamp: "2026-10-26T10:00:00Z",
  message: "floyd",
  localId: 1,
  snippet: "<mark>floyd</mark>"
};

const answers = (url: URL) => {
  if (url.pathname.endsWith("/rooms/available")) return [];
  if (url.pathname.endsWith("/rooms/history")) return { rooms: [] };
  if (url.pathname.endsWith("/users")) return { users: [], total: 0 };
  return undefined;
};

describe("a chat search", () => {
  afterEach(() => {
    chat.clear();
    vi.unstubAllGlobals();
  });

  const searches = (net: { requests: { url: URL }[] }) =>
    net.requests.filter((r) => r.url.pathname.endsWith("/messages/search"));

  it("reads a text once, so Enter can wait on the read the typing started", async () => {
    const net = fakeNetwork((url) => answers(url) ?? { hits: [hit], total: 1 });
    const search = new ChatSearch(
      () => "floyd",
      () => "all",
      20
    );
    await Promise.all([search.read(), search.settled(), search.read()]);
    expect(searches(net).length).toBe(1);
    expect(search.current).toBe(true);
    expect(search.hits).toEqual([hit]);
  });

  it("reads a query the daemon refuses again as plain words, with no error", async () => {
    const net = fakeNetwork((url) => {
      const known = answers(url);
      if (known) return known;
      return url.searchParams.get("q") === '"floyd"*'
        ? { hits: [hit], total: 1 }
        : { status: 400, body: { error: "The value of q is not a valid search" } };
    });
    const search = new ChatSearch(
      () => '"floyd',
      () => "all",
      20
    );
    await search.read();
    expect(searches(net).map((r) => r.url.searchParams.get("q"))).toEqual(['"floyd', '"floyd"*']);
    expect(search.hits).toEqual([hit]);
    expect(search.error).toBe("");
  });
});
