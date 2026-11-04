import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";

import { chat } from "./chat.svelte";
import { MessagePane } from "./pane.svelte";

const message = (localId: number) => ({
  localId,
  timestamp: new Date(Date.UTC(2026, 9, 27, 18, 0, localId)).toISOString(),
  username: "alice",
  message: `line ${localId}`,
  roomName: "indie",
  self: false,
  isMention: false,
  hidden: false
});

describe("a room's pane opened by a link", () => {
  afterEach(() => {
    chat.clear();
    vi.unstubAllGlobals();
  });

  it("places the unread line from the read marker and reads to the newest", async () => {
    let answerRooms: () => void = () => {};
    const rooms = new Promise<void>((done) => (answerRooms = done));
    const net = fakeNetwork(async (url, method) => {
      if (url.pathname.endsWith("/rooms/joined")) {
        // the joined rooms come after the room's own messages
        await rooms;
        return [
          {
            name: "indie",
            unread: 3,
            mentions: 0,
            notify: "mentions",
            userCount: 3,
            lastMessageAt: null,
            lastReadId: 3
          }
        ];
      }
      if (url.pathname.endsWith("/conversations")) return [];
      if (url.pathname.endsWith("/messages")) {
        return { messages: [1, 2, 3, 4, 5, 6].map(message), hasMore: false, gaps: [] };
      }
      if (url.pathname.endsWith("/read") && method === "PUT") return { status: 204 };
      return undefined;
    });
    const pane = new MessagePane({ kind: "room", name: "indie" }, () => "me");
    const loading = pane.load();
    await new Promise((resolve) => setTimeout(resolve, 0));
    answerRooms();
    await loading;
    expect(pane.unreadFrom).toBe(4);
    await pane.markRead();
    expect(net.requests.filter((r) => r.method === "PUT").map((r) => r.body)).toEqual([
      { localId: 6 }
    ]);
  });
});

describe("a pane opened at a message", () => {
  afterEach(() => {
    chat.clear();
    vi.unstubAllGlobals();
  });

  /** A room of `total` messages; `after` and `before` page through it as the daemon does */
  const stored = (total: number) => {
    const asked: string[] = [];
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/rooms/joined")) return [];
      if (url.pathname.endsWith("/conversations")) return [];
      if (!url.pathname.endsWith("/messages")) return undefined;
      asked.push(url.search);
      const limit = Number(url.searchParams.get("limit"));
      const ids = Array.from({ length: total }, (_, n) => n + 1);
      const after = url.searchParams.get("after");
      if (after !== null) {
        const past = ids.filter((id) => id > Number(after));
        return {
          messages: past.slice(0, limit).map(message),
          hasMore: past.length > limit,
          gaps: []
        };
      }
      const before = Number(url.searchParams.get("before") ?? total + 1);
      const up = ids.filter((id) => id < before);
      return { messages: up.slice(-limit).map(message), hasMore: up.length > limit, gaps: [] };
    });
    return asked;
  };
  const ids = (pane: MessagePane) => pane.ordered.map((m) => m.localId);
  const room = { kind: "room", name: "indie" } as const;

  it("holds the pages around it, then reads down to the newest", async () => {
    stored(450);
    const pane = new MessagePane(room, () => "me", 150);
    await pane.load();
    expect(ids(pane)).toEqual(Array.from({ length: 200 }, (_, n) => n + 51));
    expect([pane.hasMore, pane.hasNewer]).toEqual([true, true]);

    await pane.newer();
    expect(ids(pane).at(-1)).toBe(350);
    expect(pane.hasNewer).toBe(true);
    await pane.newer();
    expect(ids(pane).at(-1)).toBe(450);
    expect(pane.hasNewer).toBe(false);
  });

  it("is at the newest when its message is near them", async () => {
    stored(120);
    const pane = new MessagePane(room, () => "me", 100);
    await pane.load();
    expect(ids(pane).at(-1)).toBe(120);
    expect([pane.hasMore, pane.hasNewer]).toEqual([false, false]);
  });

  it("leaves a message that arrives to its place past the pages held", async () => {
    stored(450);
    const pane = new MessagePane(room, () => "me", 150);
    await pane.load();
    const held = ids(pane).length;
    const asked = stored(450);
    await pane.load();
    expect(asked, "a pane short of the newest reads nothing on a refresh").toEqual([]);
    expect(ids(pane).length).toBe(held);
  });

  it("goes to the newest page before a message is sent", async () => {
    stored(450);
    const pane = new MessagePane(room, () => "me", 150);
    await pane.load();
    await pane.send("hello");
    expect(pane.hasNewer).toBe(false);
    expect(pane.returned).toBe(1);
    expect(pane.ordered.at(-1)?.text).toBe("hello");
    expect(ids(pane).slice(0, -1)).toEqual(Array.from({ length: 100 }, (_, n) => n + 351));
  });

  it("drops a page of the older stretch that lands after going to the newest", async () => {
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    stored(450);
    const pane = new MessagePane(room, () => "me", 150);
    await pane.load();
    // the next page down is slow to come, and the user jumps to the newest meanwhile
    const network = globalThis.fetch;
    vi.stubGlobal("fetch", async (input: Request) => {
      if (new URL(input.url).searchParams.has("after")) await held;
      return network(input);
    });
    const down = pane.newer();
    await pane.latest();
    release();
    await down;
    expect(ids(pane)).toEqual(Array.from({ length: 100 }, (_, n) => n + 351));
    expect(pane.hasNewer).toBe(false);
  });

  it("opens empty on a room known by name with nothing stored", async () => {
    fakeNetwork((url) => (url.pathname.endsWith("/rooms/joined") ? [] : undefined));
    const pane = new MessagePane({ kind: "room", name: "new" }, () => "me");
    await pane.load();
    expect(pane.error).toBe("");
    expect(pane.ordered).toEqual([]);
  });
});
