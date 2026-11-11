import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { chat, type JoinedRoom } from "#lib/chat/chat.svelte.js";
import { ECHO_WAIT } from "#lib/chat/pane.svelte.js";
import { chatLayout } from "#lib/chat/prefs.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { conversation, joinedRoom } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Pane from "./Pane.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn(), beforeNavigate: () => {} }));

/** A minute past ten on the test day for each id, so ids past 59 are times too */
const minute = (localId: number) => new Date(Date.UTC(2026, 9, 26, 10, localId)).toISOString();

const privateMessage = (localId: number, fields: Partial<Schemas["PrivateMessage"]> = {}) =>
  ({
    localId,
    id: localId,
    timestamp: minute(localId),
    username: "alice",
    direction: "In",
    message: `hello ${localId}`,
    isAcknowledged: true,
    wasReplayed: false,
    autoReply: false,
    ...fields
  }) as Schemas["PrivateMessage"];

const roomMessage = (localId: number, fields: Partial<Schemas["RoomMessage"]> = {}) =>
  ({
    localId,
    timestamp: minute(localId),
    username: "bob",
    message: `room ${localId}`,
    roomName: "indie",
    self: false,
    isMention: false,
    hidden: false,
    ...fields
  }) as Schemas["RoomMessage"];

/** The joined room indie, `fields` over it, whose stored messages `messages` answers */
const indie = (messages: (url: URL) => unknown, fields: Partial<JoinedRoom> = {}) => {
  chat.rooms.upsert(joinedRoom("indie", { userCount: 2, ...fields }));
  return fakeNetwork((url, method) => {
    if (url.pathname.endsWith("/rooms/joined/indie/messages") && method === "GET") {
      return messages(url);
    }
    if (url.pathname.endsWith("/rooms/joined/indie")) {
      return { name: "indie", isPrivate: false, users: [], tickers: [], messages: null };
    }
    if (method === "POST") return { status: 201 };
    return undefined;
  });
};

describe("a chat pane", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    session.principal = { name: "me", role: "Administrator" } as never;
    app.state = { user: { username: "me" }, server: { isConnected: true } } as never;
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });
  afterEach(() => {
    vi.useRealTimers();
    if (component) unmount(component);
    component = null;
    live.stop();
    chat.clear();
    session.principal = null;
    app.state = null;
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  /** Lets pending fetches and zero-delay timers run under fake timers, then updates the page */
  const step = async () => {
    for (let i = 0; i < 4; i++) await vi.advanceTimersByTimeAsync(0);
    flushSync();
  };
  const rows = () => [...document.querySelectorAll<HTMLElement>("[data-message]")];
  const ids = () => rows().map((r) => r.dataset.message);
  const row = (id: number) => document.querySelector<HTMLElement>(`[data-message="${id}"]`)!;

  it("shows the server's notices read-only, with no composer and nothing about a person", async () => {
    const notice = (localId: number) =>
      privateMessage(localId, { username: "server", message: `notice ${localId}` });
    const net = connected(
      (url) => (url.pathname.endsWith("/conversations/server/messages") ? [notice(1)] : undefined),
      "Administrator"
    );
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "user", name: "server" } }
    });
    await flush();
    expect(ids()).toEqual(["1"]);
    expect(document.querySelector("h2")?.textContent).toBe("Server");
    expect(document.querySelector("textarea")).toBeNull();
    expect(document.querySelector("[aria-label='Quote']")).toBeNull();
    expect(row(1).textContent).not.toContain("server");
    expect(row(1).querySelector("a")).toBeNull();
    expect(row(1).querySelector("time")?.className).not.toContain("invisible");

    // a frame is built before the notice is acknowledged, which must not read as unread: the
    // pane at the newest messages would acknowledge an unread one
    const fresh = { ...notice(2), isAcknowledged: false };
    net.frame("conversations.message", { username: "server", message: fresh });
    await flush();
    expect(ids()).toEqual(["1", "2"]);
    expect(net.requests.filter((r) => r.method !== "GET")).toEqual([]);
  });

  it("adds a conversation's new message without rebuilding the rows or losing a selection", async () => {
    const unread = [privateMessage(1), privateMessage(2, { isAcknowledged: false })];
    const net = connected((url, method) => {
      if (url.pathname.endsWith("/conversations/alice/messages")) return unread;
      if (url.pathname.endsWith("/conversations/alice") && method === "GET") return { status: 404 };
      if (method === "PUT") return { status: 200 };
      if (method === "POST") return { status: 201 };
      return undefined;
    }, "Administrator");
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
    expect(document.body.textContent).toContain("Sending");
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toBe("hi back");
    net.frame("conversations.message", {
      username: "alice",
      message: privateMessage(4, { direction: "Out", id: null, message: "hi back" })
    });
    await flush();
    expect(ids()).toEqual(["1", "2", "3", "4"]);
    expect(document.body.textContent).not.toContain("Sending");
    expect(chat.total).toBe(0);
  });

  // two pages of a hundred messages, as a short page would say nothing newer is left
  it("acknowledges a conversation opened at an old message only once the newest are shown", async () => {
    vi.useFakeTimers();
    // jsdom measures every line as 0 high, which would draw them all
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(24);
    const stored = 200;
    const messages = (from: number, to: number) =>
      Array.from({ length: Math.max(0, to - from + 1) }, (_, n) =>
        privateMessage(from + n, { isAcknowledged: false })
      );
    const net = connected((url, method) => {
      if (method === "PUT") return { status: 200 };
      if (url.pathname.endsWith("/conversations/alice")) return { status: 404 };
      if (!url.pathname.endsWith("/conversations/alice/messages")) return undefined;
      const limit = Number(url.searchParams.get("limit"));
      const after = url.searchParams.get("after");
      if (after !== null)
        return messages(Number(after) + 1, Math.min(stored, Number(after) + limit));
      const before = Number(url.searchParams.get("before") ?? stored + 1);
      return messages(Math.max(1, before - limit), before - 1);
    }, "Administrator");
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "user", name: "alice" }, at: 3 }
    });
    await step();
    expect(net.requests.some((r) => r.url.searchParams.get("after") === "3")).toBe(true);
    await step();
    const acknowledged = () => net.requests.filter((r) => r.method === "PUT");
    expect(acknowledged()).toEqual([]);

    [...document.querySelectorAll("button")]
      .find((b) => b.textContent?.includes("Jump to latest"))!
      .click();
    await step();
    expect(acknowledged().map((r) => r.url.pathname)).toEqual(["/api/v0/conversations/alice"]);
  });

  it("pages a room's history up, with its gaps as rows, and takes new messages in place", async () => {
    const older = { messages: [roomMessage(1)], hasMore: false, gaps: [] };
    const newest = {
      messages: [roomMessage(10), roomMessage(11)],
      hasMore: true,
      gaps: [{ from: "2026-10-26T10:10:30Z", to: "2026-10-26T10:10:50Z" }]
    };
    const net = indie((url) => (url.searchParams.get("before") ? older : newest));
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "room", name: "indie" } }
    });
    await flush();
    expect(ids()).toEqual(["10", "11"]);
    expect(document.querySelectorAll("[role=note]")).toHaveLength(1);
    expect(document.body.textContent).toContain("(disconnected from Soulseek)");
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
    net.frame("rooms.gap", { roomName: "indie", gap: { from: "2026-10-26T10:13:00Z", to: null } });
    await flush();
    expect(ids()).toEqual(["1", "10", "11", "12"]);
    expect(row(10)).toBe(ten);
    expect(document.querySelectorAll("[role=note]")).toHaveLength(2);
  });

  it("adds no gap from a login to the history of another room", async () => {
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/rooms/history/indie/messages")
        ? { messages: [roomMessage(1)], hasMore: false, gaps: [] }
        : undefined
    );
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "room", name: "indie" } }
    });
    await flush();
    expect(ids()).toEqual(["1"]);

    net.frame("rooms.gap", { roomName: "jazz", gap: { from: "2026-10-26T10:13:00Z", to: null } });
    await flush();
    expect(document.querySelectorAll("[role=note]")).toHaveLength(0);
  });

  it("keeps joins and leaves out of the messages and folds them in the room's activity", async () => {
    const net = indie(() => ({ messages: [roomMessage(1)], hasMore: false, gaps: [] }), {
      lastReadId: 1
    });
    chatLayout.users = true;
    chatLayout.side = "activity";
    live.start();
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "room", name: "indie" } }
    });
    await flush();
    const joined = (username: string) =>
      net.frame("rooms.user_joined", {
        roomName: "indie",
        user: { username, status: "Online" } as Schemas["RoomUser"]
      });
    joined("carol");
    await flush();
    const grid = () => document.querySelector("[aria-label='Joins, leaves and status changes']")!;
    const lines = () => [...grid().querySelectorAll("[role=row]")].map((r) => r.ariaLabel);
    expect(lines()).toEqual(["carol joined"]);
    const first = grid().querySelector("[role=row]");
    // a second join soon after folds into the same row; a leave starts a new one above it
    joined("dave");
    net.frame("rooms.user_left", { roomName: "indie", username: "carol" });
    await flush();
    expect(lines()).toEqual(["carol left", "carol and dave joined"]);
    expect(grid().querySelectorAll("[role=row]")[1]).toBe(first);
    const log = document.querySelector("[role=log]")!;
    expect(ids()).toEqual(["1"]);
    expect(log.textContent).not.toMatch(/joined|left/);
  });

  it("shows a room message the server never echoed as not sent, and takes a late echo", async () => {
    vi.useFakeTimers();
    const net = indie(() => ({ messages: [roomMessage(1)], hasMore: false, gaps: [] }));
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
    // the daemon took it and the echo is still to come: the row stays, no longer sending
    expect(net.requests.some((r) => r.method === "POST")).toBe(true);
    expect(ids()).toHaveLength(2);
    expect(document.body.textContent).not.toContain("Sending");
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
  });

  it("puts the unread line after the room's read marker", async () => {
    indie(
      () => ({
        messages: [roomMessage(1), roomMessage(2), roomMessage(3, { self: true }), roomMessage(4)],
        hasMore: false,
        gaps: []
      }),
      { unread: 1, lastReadId: 2 }
    );
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

  it("offers the names in the chat for @, ourselves left out and each once", async () => {
    const member = (username: string) => ({ username, self: username === "me" }) as never;
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/conversations/alice/messages")) return [];
      if (url.pathname.endsWith("/rooms/joined/indie/messages")) {
        return { messages: [], hasMore: false, gaps: [] };
      }
      if (url.pathname.endsWith("/rooms/joined/indie")) {
        const users = [member("me"), member("mel")];
        return { name: "indie", isPrivate: false, users, tickers: [], messages: null };
      }
      return undefined;
    });
    chat.rooms.upsert(joinedRoom("indie"));
    chat.conversations.upsert(conversation("alice"));
    chat.conversations.upsert(conversation("me"));
    const offered = async (target: { kind: "room" | "user"; name: string }) => {
      component = mount(Pane, { target: document.body, props: { target } });
      await flush();
      const box = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]')!;
      box.value = "@";
      box.setSelectionRange(1, 1);
      box.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      const names = [...document.querySelectorAll('[aria-label="Usernames"] [role="option"]')];
      unmount(component);
      component = null;
      return names.map((o) => o.textContent?.trim());
    };
    expect(await offered({ kind: "room", name: "indie" })).toEqual(["mel"]);
    expect(await offered({ kind: "user", name: "alice" })).toEqual(["alice"]);
  });

  it("keeps a user's card open while the list moves itself for a new message", async () => {
    const net = connected((url, method) => {
      if (url.pathname.endsWith("/conversations/alice/messages")) return [privateMessage(1)];
      if (url.pathname.endsWith("/users/alice")) return { username: "alice", known: false };
      if (method === "PUT") return { status: 200 };
      return undefined;
    }, "Administrator");
    component = mount(Pane, {
      target: document.body,
      props: { target: { kind: "user", name: "alice" } }
    });
    await flush();
    const card = () => document.querySelector('[role="dialog"][aria-label="Profile of alice"]');
    // the name, not the face beside it, which links to the profile
    row(1).querySelector<HTMLAnchorElement>("a:not([aria-hidden])")!.click();
    await flush();
    expect(card()).not.toBeNull();

    net.frame("conversations.message", { username: "alice", message: privateMessage(2) });
    await flush();
    expect(ids()).toEqual(["1", "2"]);
    // a list at its end keeps to it as the new line is measured, a scroll the browser announces
    // and jsdom, which lays nothing out, does not
    document.querySelector('[role="log"]')!.dispatchEvent(new Event("scroll"));
    await flush();
    expect(card()).not.toBeNull();
  });
});
