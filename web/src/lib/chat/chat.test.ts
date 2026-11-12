import { afterEach, describe, expect, it, vi } from "vitest";

import { toasts } from "#lib/feedback/toast.svelte.js";
import { DEBOUNCE_MS, live } from "#lib/live/live.svelte.js";
import { conversation, joinedRoom } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { users } from "#lib/users/users.svelte.js";

import { chat } from "./chat.svelte";
import { ChatSearch } from "./search/search.svelte";

/** a notice from the server */
const notice = {
  localId: 3,
  id: 3,
  timestamp: "2026-10-26T10:00:00Z",
  username: "server",
  direction: "In",
  message: "user bob has not enabled private room add",
  isAcknowledged: false,
  wasReplayed: false,
  autoReply: false
} as const;

/** a message said in `indie`, stored as `localId`, the later the larger */
const roomMessage = (localId: number, hidden: boolean) => ({
  localId,
  timestamp: new Date(Date.UTC(2026, 9, 26, 10, 0, localId % 60)).toISOString(),
  username: "alice",
  message: "hi",
  roomName: "indie",
  self: false,
  isMention: false,
  hidden
});

describe("the chat store", () => {
  afterEach(() => {
    live.stop();
    chat.clear();
    users.clear();
    vi.unstubAllGlobals();
  });

  it("reads the conversation peers' summaries with the conversations, on any screen", async () => {
    fakeNetwork((url) => {
      if (url.pathname.endsWith("/api/v0/conversations")) {
        return ["alice", "bob"].map((username) => ({
          username,
          isActive: true,
          isServerNotice: false,
          unAcknowledgedMessageCount: 0,
          hasUnAcknowledgedMessages: false,
          notify: "all",
          draft: "",
          lastMessage: null,
          messages: null
        }));
      }
      if (url.pathname.endsWith("/rooms/joined")) return [];
      const summary = /\/users\/(\w+)\/summary$/.exec(url.pathname);
      return summary ? { username: summary[1], muted: summary[1] === "bob" } : undefined;
    });
    await chat.load();
    await vi.waitFor(() => expect(users.get("bob")?.muted).toBe(true));
    expect(users.get("alice")?.muted).toBe(false);
  });

  it("holds the server's notices apart from the conversations", async () => {
    const net = connected(() => undefined, "Administrator");
    net.frame("conversations.message", { username: "server", message: notice });
    net.frame("conversations.update", {
      conversation: { ...conversation("server"), isServerNotice: true, lastMessage: notice }
    });
    await flush();
    expect(chat.latestNotice?.localId).toBe(3);
    expect(chat.conversations.rows).toEqual([]);
    // no "New message from server" toast: the daemon's push of the notice is what announces it
    expect(toasts).toEqual([]);

    const found = new ChatSearch(
      () => "serv",
      () => "all",
      10
    );
    expect(found.found.users).toEqual([]);
    expect(net.requests.map((r) => r.url.pathname)).not.toContain("/api/v0/users/server/summary");

    net.frame("conversations.update", {
      conversation: { ...conversation("server"), isServerNotice: true, isActive: false }
    });
    await flush();
    expect(chat.latestNotice).toBeNull();
  });

  it("keeps a notice that comes while a read from before it is on its way", async () => {
    // the read started before the notice was stored, so it finds no conversation
    let answer = (_reply: undefined) => {};
    const net = connected(
      (url) =>
        url.pathname.endsWith("/conversations/server")
          ? new Promise((resolve) => (answer = resolve))
          : undefined,
      "Administrator"
    );
    net.frame("conversations.list", []);
    await flush();
    net.frame("conversations.message", { username: "server", message: notice });
    await flush();
    expect(chat.latestNotice?.localId).toBe(3);
    answer(undefined);
    await flush();
    expect(chat.latestNotice?.localId).toBe(3);
  });

  it("keeps a conversation that opens while a read from before it is on its way", async () => {
    let answer: ((list: unknown) => void) | undefined;
    const net = connected((url) => {
      if (url.pathname.endsWith("/api/v0/conversations")) {
        return new Promise((resolve) => (answer = resolve));
      }
      return url.pathname.endsWith("/rooms/joined") ? [] : undefined;
    }, "Administrator");
    const reading = chat.load();
    await vi.waitFor(() => expect(answer).toBeDefined());
    // carol writes for the first time after the daemon read the list
    net.frame("conversations.update", { conversation: conversation("carol") });
    await vi.waitFor(() => expect(chat.conversations.get("carol")).toBeDefined());
    answer!([conversation("alice")]);
    await reading;
    expect(chat.conversations.rows.map((c) => c.username)).toEqual(["carol", "alice"]);
  });

  it("reads a notice a stream that resumed to add the conversations never sends", async () => {
    let stored: typeof notice | null = null;
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/conversations/server")) {
        return stored
          ? { ...conversation("server"), isServerNotice: true, lastMessage: stored }
          : undefined;
      }
      if (url.pathname.endsWith("/conversations") || url.pathname.endsWith("/rooms/joined")) {
        return [];
      }
      return undefined;
    });
    vi.useFakeTimers();
    const greet = (resumed: boolean) =>
      net.frame("hello", { principal: { name: "me", role: "Administrator" }, resumed } as never);
    const release = live.retain("application");
    try {
      live.start();
      greet(false);
      chat.start();
      await vi.advanceTimersByTimeAsync(0);
      expect(chat.latestNotice).toBeNull();
      // the notice's frames go to the stream without the conversations, and the one that adds
      // them resumes past the notice's own frame
      stored = notice;
      net.frame("application.server_message", {
        message: notice.message,
        receivedAt: notice.timestamp
      });
      net.frame("metrics.update", {} as never);
      await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
      greet(true);
      await vi.advanceTimersByTimeAsync(0);
      expect(chat.latestNotice?.localId).toBe(3);
    } finally {
      release();
      vi.useRealTimers();
    }
  });

  it("counts what is read while the stream is down from the rows it keeps", async () => {
    fakeNetwork((url, method) => (method === "PUT" ? { status: 204 } : undefined));
    // counts a stream sent before it went down
    chat.unread = {
      conversations: { alice: 1 },
      rooms: { indie: { unread: 3, mentions: 1 } },
      total: 2,
      totalMentions: 1
    };
    chat.conversations.upsert({
      ...conversation("alice"),
      unAcknowledgedMessageCount: 1,
      hasUnAcknowledgedMessages: true
    });
    chat.rooms.upsert(
      joinedRoom("indie", { unread: 3, mentions: 1, lastMessage: roomMessage(7, false) })
    );
    await chat.acknowledge("alice");
    await chat.markRoomRead("indie", 7);
    expect(chat.total).toBe(0);
    expect(chat.roomUnread("indie")).toEqual({ unread: 0, mentions: 0 });
  });

  it("leaves a joined room's row on its newest message when an older one comes", async () => {
    const net = connected(() => undefined, "Administrator");
    chat.rooms.upsert(joinedRoom("indie"));
    net.frame("rooms.message", { roomName: "indie", message: roomMessage(901, false) });
    net.frame("rooms.message", { roomName: "indie", message: roomMessage(900, false) });
    await flush();
    expect(chat.rooms.get("indie")?.lastMessage?.localId).toBe(901);
  });

  it("says why each read failed when both did", async () => {
    fakeNetwork((url) => ({
      status: 500,
      body: { error: url.pathname.endsWith("/conversations") ? "locked" : "not logged in" }
    }));
    await chat.load();
    expect(chat.error).toBe("Conversations: locked. Rooms: not logged in");
  });
});
