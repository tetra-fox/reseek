import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import SearchDetail from "./SearchDetail.svelte";

const ID = "6f1c9a52-0d1e-4c43-9d55-3a1f0b6e7a10";

const file = (filename: string): Schemas["File"] => ({
  filename,
  size: 4_000_000,
  code: 1,
  extension: "flac",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

const response = (username: string, speed: number): Schemas["SearchResponse"] => ({
  username,
  token: 1,
  hasFreeUploadSlot: true,
  uploadSpeed: speed,
  queueLength: 0,
  fileCount: 2,
  files: [file(`@@${username}\\Album\\01 One.flac`), file(`@@${username}\\Album\\02 Two.flac`)],
  lockedFileCount: 0,
  lockedFiles: []
});

const search = (fields: Partial<Schemas["Search"]>): Schemas["Search"] => ({
  id: ID,
  searchText: "album",
  token: 1,
  state: "InProgress",
  isComplete: false,
  startedAt: "2026-10-26T10:00:00Z",
  endedAt: null,
  responseCount: 0,
  fileCount: 0,
  lockedFileCount: 0,
  responseBytes: 0,
  responses: null,
  scope: { type: "network", subjects: [] },
  ...fields
});

const flush = async () => {
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
};

describe("search results", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    if (component) unmount(component);
    live.stop();
    searches.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("stream in without replacing the cards already shown, also once stored", async () => {
    const stored = [response("alice", 300), response("bob", 200), response("carol", 100)];
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/api/v0/users")) return { users: [], total: 0 };
      if (url.pathname.endsWith(`/searches/${ID}/responses`)) return stored;
      if (url.pathname.endsWith(`/searches/${ID}`)) return search({});
      return undefined;
    });
    searches.list.upsert(search({}));
    component = mount(SearchDetail, { target: document.body, props: { id: ID } });
    live.start();
    net.frame("hello", {
      principal: { name: "a", role: "ReadOnly" }
    } as unknown as Schemas["Hello"]);
    await flush();

    net.frame("search.response", { searchId: ID, response: stored[0] });
    net.frame("search.response", { searchId: ID, response: stored[1] });
    await flush();
    const cards = () => [...document.querySelectorAll("section[data-key]")];
    expect(cards().map((c) => c.getAttribute("data-key"))).toEqual(["0:alice", "0:bob"]);
    const [alice, bob] = cards();
    const row = alice.querySelector("tbody tr")!;

    // a selection across the two cards
    const selection = document.getSelection()!;
    selection.setBaseAndExtent(alice, 0, bob, bob.childNodes.length);
    const selected = selection.toString();
    expect(selected).toContain("01 One.flac");

    net.frame("search.response", { searchId: ID, response: stored[2] });
    await flush();
    expect(cards()).toHaveLength(3);
    expect(cards()[0]).toBe(alice);
    expect(cards()[1]).toBe(bob);
    expect(alice.querySelector("tbody tr")).toBe(row);
    expect(document.getSelection()!.toString()).toBe(selected);

    // completed: the stored responses replace the streamed ones, and the cards stay
    net.frame(
      "search.update",
      search({
        state: "Completed, TimedOut",
        isComplete: true,
        endedAt: "2026-10-26T10:00:15Z",
        responseCount: 3,
        fileCount: 6
      })
    );
    await flush();
    await flush();
    expect(net.requests.map((r) => r.url.pathname)).toContain(`/api/v0/searches/${ID}/responses`);
    expect(cards()).toHaveLength(3);
    expect(cards()[0]).toBe(alice);
    expect(cards()[1]).toBe(bob);
    expect(alice.querySelector("tbody tr")).toBe(row);
    expect(document.getSelection()!.toString()).toBe(selected);
  });
});
