import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { chat } from "#lib/chat/chat.svelte.js";
import { session } from "#lib/session.svelte.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import ChatList from "./ChatList.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const conversation = (username: string) => ({
  username,
  isActive: true,
  isServerNotice: false,
  unAcknowledgedMessageCount: 0,
  hasUnAcknowledgedMessages: false,
  notify: "all" as const,
  draft: "",
  lastMessage: null,
  messages: null
});
const room = (name: string, lastMessageAt: string) => ({
  name,
  unread: 0,
  mentions: 0,
  notify: "mentions" as const,
  userCount: 3,
  lastMessageAt,
  lastReadId: 0
});

describe("the chat list", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    chat.clear();
    session.principal = null;
    vi.unstubAllGlobals();
    vi.mocked(goto).mockClear();
    document.body.innerHTML = "";
  });

  const start = () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    fakeNetwork(() => undefined);
    chat.rooms.reconcile([
      room("indie", "2026-10-26T10:00:00Z"),
      room("jazz", "2026-10-26T11:00:00Z")
    ]);
    chat.conversations.reconcile([conversation("alice"), conversation("bob")]);
    component = mount(ChatList, {
      target: document.body,
      props: { current: null, onjoin: () => {}, onnew: () => {} }
    });
    flushSync();
  };
  const grid = () => document.querySelector("[role=grid]")!;

  it("lists rooms by their last message, then conversations, and moves past the headers", () => {
    start();
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["h:rooms", "room:jazz", "room:indie", "h:conversations", "user:alice", "user:bob"]);
    press(grid(), "ArrowDown");
    expect(active(grid())).toBe("room:jazz");
    press(grid(), "ArrowDown");
    press(grid(), "ArrowDown");
    expect(active(grid())).toBe("user:alice");
    press(grid(), "Enter");
    expect(goto).toHaveBeenCalledWith("#/chat/alice");
  });

  it("selects with Space and Ctrl+A within a section, and clears with Escape", () => {
    start();
    press(grid(), "ArrowDown");
    press(grid(), " ");
    expect(ticked()).toEqual(["room:jazz"]);
    press(grid(), "a", { metaKey: true });
    expect(ticked()).toEqual(["room:jazz", "room:indie"]);
    expect(document.body.textContent).toContain("2 selected");
    press(grid(), "Escape");
    expect(ticked()).toEqual([]);
  });

  it("opens a chat with a click and toggles one with Ctrl-click", () => {
    start();
    const line = (key: string) => document.querySelector(`[data-line="${key}"]`)!;
    line("user:bob").dispatchEvent(new MouseEvent("click", { bubbles: true }));
    flushSync();
    expect(goto).toHaveBeenCalledWith("#/chat/bob");
    expect(ticked()).toEqual(["user:bob"]);
    vi.mocked(goto).mockClear();
    line("room:indie").dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
    flushSync();
    expect(goto).not.toHaveBeenCalled();
    expect(ticked()).toEqual(["room:indie", "user:bob"]);
  });
});
