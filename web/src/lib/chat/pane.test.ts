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
