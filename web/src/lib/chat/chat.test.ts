import { afterEach, describe, expect, it, vi } from "vitest";

import { toasts } from "#lib/feedback/toast.svelte.js";
import { DEBOUNCE_MS, live } from "#lib/live/live.svelte.js";
import { conversation } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { users } from "#lib/users/users.svelte.js";

import { chat } from "./chat.svelte";
import { ChatSearch } from "./search/search.svelte";

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
    const notice = {
      localId: 3,
      id: 3,
      timestamp: "2026-10-26T10:00:00Z",
      username: "server",
      direction: "In",
      message: "User bob is now a member of room jazz",
      isAcknowledged: false,
      wasReplayed: false
    } as const;
    const net = fakeNetwork(() => undefined);
    live.start();
    net.frame("hello", { principal: { name: "me", role: "Administrator" } } as never);
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
    const notice = {
      localId: 3,
      id: 3,
      timestamp: "2026-10-26T10:00:00Z",
      username: "server",
      direction: "In",
      message: "user bob has not enabled private room add",
      isAcknowledged: false,
      wasReplayed: false
    } as const;
    // the read started before the notice was stored, so it finds no conversation
    let answer = (_reply: undefined) => {};
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/conversations/server")
        ? new Promise((resolve) => (answer = resolve))
        : undefined
    );
    live.start();
    net.frame("hello", { principal: { name: "me", role: "Administrator" } } as never);
    net.frame("conversations.list", []);
    await flush();
    net.frame("conversations.message", { username: "server", message: notice });
    await flush();
    expect(chat.latestNotice?.localId).toBe(3);
    answer(undefined);
    await flush();
    expect(chat.latestNotice?.localId).toBe(3);
  });

  it("reads a notice a stream that resumed to add the conversations never sends", async () => {
    const notice = {
      localId: 3,
      id: 3,
      timestamp: "2026-10-26T10:00:00Z",
      username: "server",
      direction: "In",
      message: "user bob has not enabled private room add",
      isAcknowledged: false,
      wasReplayed: false
    } as const;
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

  it("says why each read failed when both did", async () => {
    fakeNetwork((url) => ({
      status: 500,
      body: { error: url.pathname.endsWith("/conversations") ? "locked" : "not logged in" }
    }));
    await chat.load();
    expect(chat.error).toBe("Conversations: locked. Rooms: not logged in");
  });
});
