import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { chat } from "./chat.svelte";
import { KEPT, MessagePane, PAGE } from "./pane.svelte";

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

/**
 * A room of `total` messages, which a message arriving raises; `after` and `before` page
 * through it as the daemon does
 */
const stored = (total: number) => {
  const asked: string[] = [];
  const held = { asked, total, frame: (() => {}) as ReturnType<typeof fakeNetwork>["frame"] };
  const net = fakeNetwork((url) => {
    if (url.pathname.endsWith("/rooms/joined")) return [];
    if (url.pathname.endsWith("/conversations")) return [];
    if (!url.pathname.endsWith("/messages")) return undefined;
    asked.push(url.search);
    const limit = Number(url.searchParams.get("limit"));
    const ids = Array.from({ length: held.total }, (_, n) => n + 1);
    const after = url.searchParams.get("after");
    if (after !== null) {
      const past = ids.filter((id) => id > Number(after));
      return {
        messages: past.slice(0, limit).map(message),
        hasMore: past.length > limit,
        gaps: []
      };
    }
    const before = Number(url.searchParams.get("before") ?? held.total + 1);
    const up = ids.filter((id) => id < before);
    return { messages: up.slice(-limit).map(message), hasMore: up.length > limit, gaps: [] };
  });
  held.frame = net.frame;
  return held;
};
/** The live stream, followed by `pane` until the returned function is called */
const following = (pane: MessagePane, frame: ReturnType<typeof stored>["frame"]) => {
  const stop = $effect.root(() => pane.follow());
  live.start();
  frame("hello", { principal: { name: "me", role: "Administrator" } } as never);
  return stop;
};
const ids = (pane: MessagePane) => pane.ordered.map((m) => m.localId);
const room = { kind: "room", name: "indie" } as const;

describe("a room's pane opened by a link", () => {
  afterEach(() => {
    chat.clear();
    vi.unstubAllGlobals();
  });

  it("places the unread line from the read marker and reads to the newest", async () => {
    let answerRooms: () => void = () => {};
    const rooms = new Promise<void>((done) => (answerRooms = done));
    let roomsAsked: () => void = () => {};
    const asked = new Promise<void>((done) => (roomsAsked = done));
    const net = fakeNetwork(async (url, method) => {
      if (url.pathname.endsWith("/rooms/joined")) {
        // the joined rooms answer only once the pane is waiting on them
        roomsAsked();
        await rooms;
        return [
          {
            name: "indie",
            unread: 3,
            mentions: 0,
            notify: "mentions",
            userCount: 3,
            lastMessageAt: null,
            lastMessage: null,
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
    await asked;
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
    live.stop();
    chat.clear();
    vi.unstubAllGlobals();
  });

  /**
   * Holds every answer about the messages until `wait` is done, or gives what it answers
   * instead; the answer is made before the wait, as the room was when it was asked
   */
  const delayed = (wait: (url: URL) => Promise<Response | undefined>) => {
    const network = globalThis.fetch;
    vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(input instanceof Request ? input.url : String(input), location.href);
      const answer = await network(input, init);
      if (!url.pathname.endsWith("/messages")) return answer;
      return (await wait(url)) ?? answer;
    });
  };

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

  it("reads only what arrived on reaching the newest, however much it is", async () => {
    const room450 = stored(450);
    const pane = new MessagePane(room, () => "me", 300);
    await pane.load();
    expect(ids(pane).at(-1)).toBe(400);
    // the last page down holds the newest, and 150 more arrive while it is read
    const network = globalThis.fetch;
    vi.stubGlobal("fetch", async (input: Request) => {
      const answer = await network(input);
      if (new URL(input.url).searchParams.get("after") === "400") room450.total = 600;
      return answer;
    });
    room450.asked.length = 0;
    await pane.newer();
    expect(room450.asked.map((q) => new URLSearchParams(q).get("after"))).toEqual(["400", "450"]);
    expect(ids(pane).at(-1)).toBe(550);
    expect(pane.hasNewer).toBe(true);
    await pane.newer();
    expect(ids(pane)).toEqual(Array.from({ length: 400 }, (_, n) => n + 201));
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
    const { asked } = stored(450);
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
      if (new URL(input.url).searchParams.get("after") === "250") await held;
      return network(input);
    });
    const down = pane.newer();
    await pane.latest();
    release();
    await down;
    expect(ids(pane)).toEqual(Array.from({ length: 100 }, (_, n) => n + 351));
    expect(pane.hasNewer).toBe(false);
  });

  it("reads the pages around it again when the first read failed", async () => {
    stored(450);
    let down = true;
    delayed(async () =>
      down
        ? new Response(JSON.stringify({ error: "reseek is not reachable" }), {
            status: 500,
            headers: { "content-type": "application/json" }
          })
        : undefined
    );
    const pane = new MessagePane(room, () => "me", 150);
    await pane.load();
    expect(pane.error).not.toBe("");
    down = false;
    await pane.load();
    expect(pane.error).toBe("");
    expect(ids(pane)).toEqual(Array.from({ length: 200 }, (_, n) => n + 51));
    expect([pane.hasMore, pane.hasNewer]).toEqual([true, true]);
  });

  it("keeps a message that arrives while its pages are read out of the stretch", async () => {
    const { frame } = stored(450);
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    delayed(async () => void (await held));
    const pane = new MessagePane(room, () => "me", 150);
    const stop = following(pane, frame);
    const loading = pane.load();
    frame("rooms.message", { roomName: "indie", message: message(451) });
    await flush();
    release();
    await loading;
    stop();
    expect(ids(pane)).toEqual(Array.from({ length: 200 }, (_, n) => n + 51));
    expect(pane.hasNewer).toBe(true);
  });

  it("reads a message that arrives while its pages are read when they reach the newest", async () => {
    const room450 = stored(450);
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    delayed(async () => void (await held));
    const pane = new MessagePane(room, () => "me", 400);
    const stop = following(pane, room450.frame);
    const loading = pane.load();
    // both pages are answered and on their way when 451 arrives
    await flush();
    room450.total = 451;
    room450.frame("rooms.message", { roomName: "indie", message: message(451) });
    await flush();
    release();
    await loading;
    stop();
    expect(ids(pane).at(-1)).toBe(451);
    expect(pane.hasNewer).toBe(false);
  });

  it("reads a message that arrives while it goes to the newest", async () => {
    const room450 = stored(450);
    const pane = new MessagePane(room, () => "me", 150);
    await pane.load();
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    // only the newest page waits
    delayed(async (url) => {
      if (!url.searchParams.has("before") && !url.searchParams.has("after")) await held;
      return undefined;
    });
    const stop = following(pane, room450.frame);
    const going = pane.latest();
    await flush();
    room450.total = 451;
    room450.frame("rooms.message", { roomName: "indie", message: message(451) });
    await flush();
    release();
    await going;
    stop();
    expect(ids(pane).at(-1)).toBe(451);
    expect(pane.hasNewer).toBe(false);
  });

  it("lets the pages around it go when the view is emptied before they come", async () => {
    stored(450);
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    delayed(async () => void (await held));
    const pane = new MessagePane(room, () => "me", 150);
    const loading = pane.load();
    await flush();
    pane.clearView();
    release();
    await loading;
    expect(ids(pane)).toEqual([]);
    expect([pane.loaded, pane.hasMore, pane.hasNewer]).toEqual([true, false, false]);
  });

  it("reads a conversation's draft when the view is emptied before the first page", async () => {
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    fakeNetwork(async (url) => {
      if (url.pathname.endsWith("/messages")) {
        await held;
        return [];
      }
      if (url.pathname.endsWith("/conversations/alice")) {
        return { username: "alice", draft: "half a thought" };
      }
      return undefined;
    });
    const pane = new MessagePane({ kind: "user", name: "alice" }, () => "me");
    const loading = pane.load();
    await flush();
    pane.clearView();
    release();
    await loading;
    await flush();
    expect(pane.conversation?.draft).toBe("half a thought");
  });

  it("opens at the newest when a message is sent before the pages around it come", async () => {
    stored(450);
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    // only the reads around the message wait
    delayed(async (url) => {
      const around =
        url.searchParams.get("before") === "151" || url.searchParams.get("after") === "150";
      if (around) await held;
      return undefined;
    });
    const pane = new MessagePane(room, () => "me", 150);
    const loading = pane.load();
    await flush();
    await pane.send("hello");
    release();
    await loading;
    expect(ids(pane).slice(0, -1)).toEqual(Array.from({ length: 100 }, (_, n) => n + 351));
    expect([pane.loaded, pane.at, pane.hasNewer]).toEqual([true, null, false]);
  });

  it("opens empty on a room known by name with nothing stored", async () => {
    fakeNetwork((url) => (url.pathname.endsWith("/rooms/joined") ? [] : undefined));
    const pane = new MessagePane({ kind: "room", name: "new" }, () => "me");
    await pane.load();
    expect(pane.error).toBe("");
    expect(pane.ordered).toEqual([]);
  });
});

describe("a pane at the newest messages", () => {
  afterEach(() => {
    live.stop();
    chat.clear();
    vi.unstubAllGlobals();
  });

  it("places messages that arrive in order, and older pages before them", async () => {
    const room450 = stored(450);
    const pane = new MessagePane(room, () => "me");
    await pane.load();
    const stop = following(pane, room450.frame);
    for (const id of [451, 453, 452]) {
      room450.frame("rooms.message", { roomName: "indie", message: message(id) });
    }
    await flush();
    stop();
    expect(ids(pane).slice(-4)).toEqual([450, 451, 452, 453]);
    await pane.older();
    expect(ids(pane)).toEqual(Array.from({ length: 2 * PAGE + 3 }, (_, n) => n + 251));
  });

  it("lets the oldest messages past what it keeps go, and reads them back", async () => {
    const total = KEPT + PAGE + 50;
    const history = stored(total);
    const pane = new MessagePane(room, () => "me");
    await pane.load();
    while (pane.hasMore) await pane.older();
    expect(ids(pane).length).toBe(total);

    pane.trim();
    expect(ids(pane)).toEqual(Array.from({ length: KEPT }, (_, n) => n + total - KEPT + 1));
    expect(pane.hasMore).toBe(true);

    const stop = following(pane, history.frame);
    history.total = total + 1;
    history.frame("rooms.message", { roomName: "indie", message: message(total + 1) });
    await flush();
    stop();
    expect(ids(pane).at(-1)).toBe(total + 1);

    await pane.older();
    expect(ids(pane)[0]).toBe(total - KEPT - PAGE + 1);
    expect(pane.hasMore).toBe(true);
    await pane.older();
    expect(ids(pane)[0]).toBe(1);
    expect(pane.hasMore).toBe(false);
  });
});

describe("a conversation's pane", () => {
  afterEach(() => {
    live.stop();
    chat.clear();
    vi.unstubAllGlobals();
  });

  it("marks a message unread again wherever it is held when the server delivers it again", async () => {
    const said = (localId: number, isAcknowledged = true) => ({
      localId,
      id: localId,
      timestamp: new Date(Date.UTC(2026, 9, 27, 18, 0, localId)).toISOString(),
      username: "alice",
      direction: "In",
      message: `line ${localId}`,
      isAcknowledged,
      wasReplayed: false,
      autoReply: false
    });
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/conversations")) return [];
      if (url.pathname.endsWith("/messages")) return [said(1), said(2)];
      return undefined;
    });
    const pane = new MessagePane({ kind: "user", name: "alice" }, () => "me");
    const stop = $effect.root(() => pane.follow());
    live.start();
    net.frame("hello", { principal: { name: "me", role: "Administrator" } } as never);
    await pane.load();
    expect(pane.ordered.map((m) => m.unread)).toEqual([false, false]);
    const reads = net.requests.length;
    // our acknowledgement of the older one was lost, so the server delivers it again
    net.frame("conversations.update", {
      conversation: { username: "alice", unAcknowledgedMessageCount: 1 },
      redelivered: said(1, false)
    } as never);
    await vi.waitFor(() => expect(pane.ordered.map((m) => m.unread)).toEqual([true, false]));
    expect(net.requests).toHaveLength(reads);
    stop();
  });
});

describe("a room's pane at the newest messages", () => {
  afterEach(() => {
    chat.clear();
    vi.unstubAllGlobals();
  });

  it("reads on from its newest message after missing more than a page", async () => {
    const held = stored(150);
    const pane = new MessagePane(room, () => "me");
    await pane.load();
    expect(ids(pane)).toEqual(Array.from({ length: PAGE }, (_, n) => n + 51));
    // 250 arrive while the pane hears of none, then it is read again
    held.total = 400;
    await pane.load();
    expect(ids(pane)).toEqual(Array.from({ length: 200 }, (_, n) => n + 51));
    expect(pane.hasNewer).toBe(true);
  });
});
