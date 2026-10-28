import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { chat } from "#lib/chat/chat.svelte.js";
import { ECHO_WAIT } from "#lib/chat/pane.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Pane from "./Pane.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn(), beforeNavigate: () => {} }));

const flush = async () => {
  for (let i = 0; i < 4; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

const privateMessage = (localId: number, fields: Partial<Schemas["PrivateMessage"]> = {}) =>
  ({
    localId,
    id: localId,
    timestamp: `2026-10-26T10:${String(localId).padStart(2, "0")}:00Z`,
    username: "alice",
    direction: "In",
    message: `hello ${localId}`,
    isAcknowledged: true,
    wasReplayed: false,
    ...fields
  }) as Schemas["PrivateMessage"];

const roomMessage = (localId: number, fields: Partial<Schemas["RoomMessage"]> = {}) =>
  ({
    localId,
    timestamp: `2026-10-26T10:${String(localId).padStart(2, "0")}:00Z`,
    username: "bob",
    message: `room ${localId}`,
    roomName: "indie",
    self: false,
    isMention: false,
    hidden: false,
    ...fields
  }) as Schemas["RoomMessage"];

describe("a chat pane", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    session.principal = { name: "me", role: "Administrator" } as never;
    app.state = { user: { username: "me" }, server: { isConnected: true } } as never;
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    chat.clear();
    session.principal = null;
    app.state = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const rows = () => [...document.querySelectorAll<HTMLElement>("[data-message]")];
  const ids = () => rows().map((r) => r.dataset.message);
  const row = (id: number) => document.querySelector<HTMLElement>(`[data-message="${id}"]`)!;

  it("adds a conversation's new message without rebuilding the rows or losing a selection", async () => {
    const unread = [privateMessage(1), privateMessage(2, { isAcknowledged: false })];
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/conversations/alice/messages")) return unread;
      if (url.pathname.endsWith("/conversations/alice") && method === "GET") return { status: 404 };
      if (method === "PUT") return { status: 200 };
      if (method === "POST") return { status: 201 };
      return undefined;
    });
    live.start();
    net.frame("hello", { principal: { name: "me", role: "Administrator" } } as never);
    net.frame("chat.unread", {
      conversations: { alice: 1 },
      rooms: {},
      total: 1,
      totalMentions: 0
    });
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "user", name: "alice" } }
    });
    await flush();
    expect(ids()).toEqual(["1", "2"]);
    expect(chat.total).toBe(1);
    // the pane at the bottom of a visible tab reads it: the conversation is acknowledged
    expect(net.requests.filter((r) => r.method === "PUT").map((r) => r.url.pathname)).toEqual([
      "/api/v0/conversations/alice"
    ]);
    net.frame("chat.unread", { conversations: {}, rooms: {}, total: 0, totalMentions: 0 });
    await flush();
    expect(chat.total).toBe(0);

    const first = row(1);
    const second = row(2);
    document.getSelection()!.setBaseAndExtent(first, 0, second, second.childNodes.length);
    net.frame("conversations.message", { username: "alice", message: privateMessage(3) });
    await flush();
    expect(ids()).toEqual(["1", "2", "3"]);
    expect(row(1)).toBe(first);
    expect(row(2)).toBe(second);
    const selection = document.getSelection()!;
    expect(selection.isCollapsed).toBe(false);
    expect(selection.anchorNode).toBe(first);
    expect(selection.focusNode).toBe(second);
    selection.removeAllRanges();

    // a message we send shows at once and goes once the daemon has it, which leaves nothing unread
    const box = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
    box.value = "hi back";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    box.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })
    );
    flushSync();
    expect(document.body.textContent).toContain("sending");
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toBe("hi back");
    net.frame("conversations.message", {
      username: "alice",
      message: privateMessage(4, { direction: "Out", id: null, message: "hi back" })
    });
    await flush();
    expect(ids()).toEqual(["1", "2", "3", "4"]);
    expect(document.body.textContent).not.toContain("sending");
    expect(chat.total).toBe(0);
  });

  it("pages a room's history up, with its gaps as rows, and takes new messages in place", async () => {
    const older = { messages: [roomMessage(1)], hasMore: false, gaps: [] };
    const newest = {
      messages: [roomMessage(10), roomMessage(11)],
      hasMore: true,
      gaps: [{ from: "2026-10-26T10:10:30Z", to: "2026-10-26T10:10:50Z", reason: "daemonDown" }]
    };
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/rooms/joined/indie/messages")) {
        return url.searchParams.get("before") ? older : newest;
      }
      if (url.pathname.endsWith("/rooms/joined/indie")) {
        return { name: "indie", isPrivate: false, users: [], tickers: [], messages: null };
      }
      return undefined;
    });
    chat.rooms.upsert({
      name: "indie",
      unread: 0,
      mentions: 0,
      notify: "mentions",
      userCount: 2,
      lastReadId: 0
    });
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "room", name: "indie" } }
    });
    await flush();
    expect(ids()).toEqual(["10", "11"]);
    expect(document.body.textContent).toContain("(reseek was not running)");
    const ten = row(10);

    // scrolled to the top: the page before the oldest message is read
    const log = document.querySelector<HTMLElement>("[role=log]")!;
    log.dispatchEvent(new Event("scroll"));
    await flush();
    const asked = net.requests.find((r) => r.url.searchParams.get("before"));
    expect(asked?.url.searchParams.get("before")).toBe("10");
    expect(ids()).toEqual(["1", "10", "11"]);
    expect(row(10)).toBe(ten);
    expect(document.body.textContent).toContain("The start of the stored history");

    net.frame("rooms.message", { roomName: "indie", message: roomMessage(12) });
    net.frame("rooms.gap", { from: "2026-10-26T10:13:00Z", to: null, reason: "disconnected" });
    await flush();
    expect(ids()).toEqual(["1", "10", "11", "12"]);
    expect(row(10)).toBe(ten);
    expect(document.body.textContent).toContain("(disconnected from Soulseek)");
  });

  it("doesn't count messages read when the pane opens away from the bottom", async () => {
    const many = Array.from({ length: 40 }, (_, n) =>
      privateMessage(n + 1, { isAcknowledged: false })
    );
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/conversations/alice/messages")) return many;
      if (url.pathname.endsWith("/conversations/alice") && method === "GET") return { status: 404 };
      if (method === "PUT") return { status: 200 };
      return undefined;
    });
    // a log 3000 px tall seen through 300 px; jsdom lays nothing out, so the sizes are given
    Object.defineProperties(HTMLElement.prototype, {
      scrollHeight: { get: () => 3000, configurable: true },
      clientHeight: { get: () => 300, configurable: true }
    });
    try {
      live.start();
      component = mount(Pane, {
        target: document.body,
        props: { target: { kind: "user", name: "alice" }, at: 3 }
      });
      await flush();
      expect(net.requests.filter((r) => r.method === "PUT").map((r) => r.url.pathname)).toEqual([]);
    } finally {
      // jsdom keeps its own on Element
      const own = HTMLElement.prototype as unknown as Record<string, unknown>;
      delete own.scrollHeight;
      delete own.clientHeight;
    }
  });

  it("keeps a room message we sent until the server's echo replaces it", async () => {
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/rooms/joined/indie/messages") && method === "GET") {
        return { messages: [roomMessage(1)], hasMore: false, gaps: [] };
      }
      if (url.pathname.endsWith("/rooms/joined/indie")) {
        return { name: "indie", isPrivate: false, users: [], tickers: [], messages: null };
      }
      if (method === "POST") return { status: 201 };
      return undefined;
    });
    chat.rooms.upsert({
      name: "indie",
      unread: 0,
      mentions: 0,
      notify: "mentions",
      userCount: 2,
      lastReadId: 0
    });
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "room", name: "indie" } }
    });
    await flush();
    const box = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
    box.value = "hello all";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    box.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })
    );
    await flush();
    // the POST answered, the echo not yet: the message stays, no longer marked as sending
    expect(net.requests.some((r) => r.method === "POST")).toBe(true);
    expect(ids()).toHaveLength(2);
    expect(Number(ids()[1])).toBeLessThan(0);
    expect(document.body.textContent).not.toContain("sending");
    net.frame("rooms.message", {
      roomName: "indie",
      message: roomMessage(2, { username: "me", self: true, message: "hello all" })
    });
    await flush();
    expect(ids()).toEqual(["1", "2"]);
  });

  it("shows a room message the server never echoed as not sent, and takes a late echo", async () => {
    vi.useFakeTimers();
    const step = async () => {
      for (let i = 0; i < 4; i++) await vi.advanceTimersByTimeAsync(0);
      flushSync();
    };
    try {
      const net = fakeNetwork((url, method) => {
        if (url.pathname.endsWith("/rooms/joined/indie/messages") && method === "GET") {
          return { messages: [roomMessage(1)], hasMore: false, gaps: [] };
        }
        if (url.pathname.endsWith("/rooms/joined/indie")) {
          return { name: "indie", isPrivate: false, users: [], tickers: [], messages: null };
        }
        if (method === "POST") return { status: 201 };
        return undefined;
      });
      chat.rooms.upsert({
        name: "indie",
        unread: 0,
        mentions: 0,
        notify: "mentions",
        userCount: 2,
        lastReadId: 0
      });
      live.start();
      component = mount(Pane, {
        target: document.body,
        props: { target: { kind: "room", name: "indie" } }
      });
      await step();
      const box = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
      box.value = "anyone here";
      box.dispatchEvent(new Event("input", { bubbles: true }));
      box.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })
      );
      await step();
      expect(document.body.textContent).not.toContain("Not sent");
      await vi.advanceTimersByTimeAsync(ECHO_WAIT);
      flushSync();
      expect(document.body.textContent).toContain("Not sent");
      expect(ids()).toHaveLength(2);
      // the echo came after all: it takes the row's place
      net.frame("rooms.message", {
        roomName: "indie",
        message: roomMessage(2, { username: "me", self: true, message: "anyone here" })
      });
      await step();
      expect(ids()).toEqual(["1", "2"]);
      expect(document.body.textContent).not.toContain("Not sent");
    } finally {
      vi.useRealTimers();
    }
  });

  it("puts the unread line after the room's read marker", async () => {
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/rooms/joined/indie/messages")) {
        return {
          messages: [
            roomMessage(1),
            roomMessage(2),
            roomMessage(3, { self: true }),
            roomMessage(4)
          ],
          hasMore: false,
          gaps: []
        };
      }
      if (url.pathname.endsWith("/rooms/joined/indie")) {
        return { name: "indie", isPrivate: false, users: [], tickers: [], messages: null };
      }
      return undefined;
    });
    chat.rooms.upsert({
      name: "indie",
      unread: 1,
      mentions: 0,
      notify: "mentions",
      userCount: 2,
      lastReadId: 2
    });
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "room", name: "indie" } }
    });
    await flush();
    const order = [...document.querySelectorAll("[data-key]")].map((l) =>
      l.getAttribute("data-key")
    );
    expect(order.indexOf("unread")).toBe(order.indexOf("m:4") - 1);
  });

  it("draws a long history in a window and keeps the drawn rows as messages arrive", async () => {
    const history = Array.from({ length: 100 }, (_, n) =>
      privateMessage(n + 1, { timestamp: `2026-10-26T10:00:${String(n % 60).padStart(2, "0")}Z` })
    );
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/conversations/alice/messages")) return history;
      if (url.pathname.endsWith("/conversations/alice") && method === "GET") return { status: 404 };
      return undefined;
    });
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "user", name: "alice" } }
    });
    await flush();
    const drawn = ids();
    expect(drawn.length).toBeGreaterThan(5);
    expect(drawn.length).toBeLessThan(100);
    const first = row(Number(drawn[0]));
    net.frame("conversations.message", { username: "alice", message: privateMessage(101) });
    await flush();
    expect(row(Number(drawn[0]))).toBe(first);
  });
});
