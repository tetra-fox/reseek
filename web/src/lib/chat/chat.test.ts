import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";
import { users } from "#lib/users.svelte.js";

import { chat } from "./chat.svelte";

describe("the chat store", () => {
  afterEach(() => {
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

  it("says why each read failed when both did", async () => {
    fakeNetwork((url) => ({
      status: 500,
      body: { error: url.pathname.endsWith("/conversations") ? "locked" : "not logged in" }
    }));
    await chat.load();
    expect(chat.error).toBe("Conversations: locked. Rooms: not logged in");
  });
});
