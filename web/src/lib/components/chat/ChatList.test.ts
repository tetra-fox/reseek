import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { chat } from "#lib/chat/chat.svelte.js";
import { pins } from "#lib/chat/pins.svelte.js";
import { Freeze, freezesWhileUsed, interaction } from "#lib/lists/interaction.svelte.js";
import { session } from "#lib/session.svelte.js";
import { conversation, joinedRoom } from "#lib/testing/fixtures.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { CHAT } from "#lib/ui/bindings.js";
import { keysAria } from "#lib/ui/keys.js";
import { users } from "#lib/users/users.svelte.js";

import ChatList from "./ChatList.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

describe("the chat list", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    chat.clear();
    users.clear();
    session.principal = null;
    vi.unstubAllGlobals();
    vi.mocked(goto).mockClear();
    document.body.innerHTML = "";
  });

  let network: ReturnType<typeof fakeNetwork>;
  const start = (role = "ReadWrite") => {
    session.principal = { name: "me", role } as never;
    network = fakeNetwork((url, _method, body) =>
      url.pathname.endsWith("/chat/pins") ? { revision: 1, pins: body } : undefined
    );
    chat.rooms.reconcile([
      joinedRoom("indie", { lastMessageAt: "2026-10-26T10:00:00Z" }),
      joinedRoom("jazz", { lastMessageAt: "2026-10-26T11:00:00Z" })
    ]);
    chat.conversations.reconcile([conversation("alice"), conversation("bob")]);
    component = mount(ChatList, {
      target: document.body,
      props: { current: null, onjoin: () => {}, onnew: () => {} }
    });
    flushSync();
  };
  const grid = () => document.querySelector("[role=grid]")!;
  const shown = () =>
    [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"));

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
    expect(goto).toHaveBeenCalledWith("#/chat/alice", { reset: false });
  });

  it("lists the server's notices apart and last, with no user and no closing", async () => {
    start();
    chat.latestNotice = {
      localId: 1,
      id: 1,
      timestamp: "2026-10-26T10:00:00Z",
      username: "server",
      direction: "In",
      message: "Room name invalid",
      isAcknowledged: true,
      wasReplayed: false,
      autoReply: false
    };
    flushSync();
    const row = document.querySelector("[data-line='user:server']")!;
    expect(
      [...document.querySelectorAll("[data-line]")]
        .map((l) => l.getAttribute("data-line"))
        .slice(-3)
    ).toEqual(["user:bob", "h:notices", "user:server"]);
    expect(row.textContent).toContain("Server");
    expect(row.textContent).toContain("Room name invalid");
    expect(row.querySelector("[style*=color]")).toBeNull();

    press(grid(), "End");
    expect(active(grid())).toBe("user:server");
    press(grid(), "Enter");
    expect(goto).toHaveBeenCalledWith("#/chat/server", { reset: false });
  });

  it("opens a chat with a click and toggles one with Ctrl-click", () => {
    start();
    const line = (key: string) => document.querySelector(`[data-line="${key}"]`)!;
    line("user:bob").dispatchEvent(new MouseEvent("click", { bubbles: true }));
    flushSync();
    expect(goto).toHaveBeenCalledWith("#/chat/bob", { reset: false });
    expect(ticked()).toEqual(["user:bob"]);
    vi.mocked(goto).mockClear();
    line("room:indie").dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
    flushSync();
    expect(goto).not.toHaveBeenCalled();
    expect(ticked()).toEqual(["room:indie", "user:bob"]);
  });

  it("orders conversations by their newest message and holds the order while held", () => {
    start();
    const keys = () =>
      [...document.querySelectorAll("[data-line^='user:']")].map((l) =>
        l.getAttribute("data-line")
      );
    expect(keys()).toEqual(["user:alice", "user:bob"]);
    const release = interaction.hold();
    chat.conversations.get("bob")!.lastMessage = {
      localId: 9,
      id: 9,
      timestamp: "2026-10-26T12:00:00Z",
      username: "bob",
      direction: "In",
      message: "hey",
      isAcknowledged: false,
      wasReplayed: false,
      autoReply: false
    };
    flushSync();
    // bob has the newest message now, but the list holds still under the pointer
    expect(keys()).toEqual(["user:alice", "user:bob"]);
    expect(document.querySelector("[data-line='user:bob']")!.textContent).toContain("hey");
    release();
    flushSync();
    expect(keys()).toEqual(["user:bob", "user:alice"]);
  });

  it("holds its order under a selection in it, and not under one in another list", () => {
    start();
    const keys = () =>
      [...document.querySelectorAll("[data-line^='user:']")].map((l) =>
        l.getAttribute("data-line")
      );
    const newest = (username: string, localId: number) => {
      chat.conversations.get(username)!.lastMessage = {
        localId,
        id: localId,
        timestamp: `2026-10-26T12:0${localId}:00Z`,
        username,
        direction: "In",
        message: "hey",
        isAcknowledged: false,
        wasReplayed: false,
        autoReply: false
      };
      flushSync();
    };
    const select = (element: Element | null) => {
      if (element) document.getSelection()!.selectAllChildren(element);
      else document.getSelection()!.removeAllRanges();
      document.dispatchEvent(new Event("selectionchange"));
      flushSync();
    };

    select(document.querySelector("[data-line='user:alice']"));
    newest("bob", 1);
    expect(keys()).toEqual(["user:alice", "user:bob"]);
    select(null);
    expect(keys()).toEqual(["user:bob", "user:alice"]);

    const other = document.body.appendChild(document.createElement("div"));
    other.textContent = "elsewhere";
    const detach = freezesWhileUsed(new Freeze())(other);
    select(other);
    newest("alice", 2);
    expect(keys()).toEqual(["user:alice", "user:bob"]);
    detach();
    select(null);
  });

  it("holds each pinned chat once above the sections, in the user's order, while joined", () => {
    start();
    chat.conversations.assign("bob", { unAcknowledgedMessageCount: 3 });
    pins.list = [
      { kind: "user", name: "bob" },
      { kind: "room", name: "indie" }
    ];
    flushSync();
    expect(shown()).toEqual([
      "h:pinned",
      "user:bob",
      "room:indie",
      "h:rooms",
      "room:jazz",
      "h:conversations",
      "user:alice"
    ]);
    expect(document.querySelector("[data-line='user:bob']")!.textContent).toContain("3");

    // a newer message moves nothing in Pinned
    chat.rooms.get("indie")!.lastMessageAt = "2026-10-26T09:00:00Z";
    chat.conversations.get("bob")!.lastMessage = null;
    flushSync();
    expect(shown().slice(0, 3)).toEqual(["h:pinned", "user:bob", "room:indie"]);

    // a pinned room we left is gone, and back in its place on joining again
    chat.rooms.remove("indie");
    flushSync();
    expect(shown().slice(0, 3)).toEqual(["h:pinned", "user:bob", "h:rooms"]);
    expect(shown()).not.toContain("room:indie");
    chat.rooms.upsert(joinedRoom("indie"));
    flushSync();
    expect(shown().slice(0, 3)).toEqual(["h:pinned", "user:bob", "room:indie"]);
  });

  it("moves a pin with Alt+Shift and the arrows, saying where it went", async () => {
    start();
    pins.list = [
      { kind: "room", name: "jazz" },
      { kind: "user", name: "alice" },
      { kind: "room", name: "indie" }
    ];
    flushSync();
    const status = () => document.querySelector("[role=status]")!.textContent;
    const move = { altKey: true, shiftKey: true };
    press(grid(), "ArrowDown");
    expect(active(grid())).toBe("room:jazz");

    expect(press(grid(), "ArrowDown", move).defaultPrevented).toBe(true);
    expect(shown().slice(1, 4)).toEqual(["user:alice", "room:jazz", "room:indie"]);
    expect(active(grid())).toBe("room:jazz");
    expect(status()).toBe("jazz moved to place 2 of 3 in Pinned");

    press(grid(), "ArrowDown", move);
    expect(shown().slice(1, 4)).toEqual(["user:alice", "room:indie", "room:jazz"]);
    press(grid(), "ArrowDown", move);
    expect(status()).toBe("jazz is already last in Pinned");
    press(grid(), "ArrowUp", move);
    expect(shown().slice(1, 4)).toEqual(["user:alice", "room:jazz", "room:indie"]);

    await vi.waitFor(() =>
      expect(network.requests.filter((r) => r.method === "PUT").at(-1)?.body).toEqual([
        { kind: "user", name: "alice" },
        { kind: "room", name: "jazz" },
        { kind: "room", name: "indie" }
      ])
    );
  });

  it("leaves the keys of a row that is not pinned, and of a reader, to the browser", () => {
    start();
    pins.list = [{ kind: "room", name: "jazz" }];
    flushSync();
    const move = { altKey: true, shiftKey: true };
    press(grid(), "ArrowDown");
    press(grid(), "ArrowDown");
    expect(active(grid())).toBe("room:indie");
    expect(press(grid(), "ArrowDown", move).defaultPrevented).toBe(false);
    unmount(component!);
    component = null;

    start("ReadOnly");
    pins.list = [
      { kind: "room", name: "jazz" },
      { kind: "user", name: "bob" }
    ];
    flushSync();
    expect(document.querySelector("button[aria-hidden=true]")).toBeNull();
    press(grid(), "ArrowDown");
    expect(press(grid(), "ArrowDown", move).defaultPrevented).toBe(false);
    expect(shown().slice(1, 3)).toEqual(["room:jazz", "user:bob"]);
  });

  it("says when the chats can't be read, and reads them again on Retry", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    let up = false;
    fakeNetwork((url) => {
      if (!up) return { status: 500, body: { error: "database is locked" } };
      if (url.pathname.endsWith("/chat/pins")) return { revision: 0, pins: [] };
      return url.pathname.endsWith("/conversations") ? [conversation("alice")] : [];
    });
    component = mount(ChatList, {
      target: document.body,
      props: { current: null, onjoin: () => {}, onnew: () => {} }
    });
    await chat.load();
    flushSync();
    expect(document.querySelector("[role=alert]")?.textContent).toContain("database is locked");
    up = true;
    [...document.querySelectorAll("button")].find((b) => b.textContent === "Retry")!.click();
    await vi.waitFor(() => expect(document.querySelector("[role=alert]")).toBeNull());
    expect(document.querySelector('[data-line="user:alice"]')).not.toBeNull();
  });

  it("opens a row's one menu from the menu key, a right-click or its button", async () => {
    start();
    const menu = () => document.querySelector("[role=menu]");
    const closes = () => document.querySelector("[role=menu]")?.textContent?.includes("Leave room");
    press(grid(), "ArrowDown");
    press(grid(), "F10", { shiftKey: true });
    await vi.waitFor(() => expect(menu()).not.toBeNull());
    expect(closes()).toBe(true);
    press(menu()!, "Escape");
    await vi.waitFor(() => expect(menu()).toBeNull());

    const alice = document.querySelector('[data-line="user:alice"]')!;
    const at = { bubbles: true, cancelable: true, clientX: 5, clientY: 5 };
    const click = new MouseEvent("contextmenu", at);
    alice.querySelector("[role=gridcell]")!.dispatchEvent(click);
    flushSync();
    expect(click.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(menu()?.textContent).toContain("Close conversation"));
    press(menu()!, "Escape");
    await vi.waitFor(() => expect(menu()).toBeNull());

    // a heading has nothing in its menu, so the browser keeps its own there
    const heading = new MouseEvent("contextmenu", at);
    document.querySelector('[data-line="h:rooms"] button')!.dispatchEvent(heading);
    flushSync();
    expect(heading.defaultPrevented).toBe(false);
    expect(menu()).toBeNull();

    // beside the rows the browser keeps its own menu, whichever row the keys are on
    const beside = new MouseEvent("contextmenu", at);
    grid().dispatchEvent(beside);
    flushSync();
    expect(beside.defaultPrevented).toBe(false);
    expect(menu()).toBeNull();

    const button = alice.querySelector<HTMLButtonElement>('[aria-label="Menu for alice"]')!;
    button.click();
    flushSync();
    expect(button.getAttribute("aria-expanded")).toBe("true");
    await vi.waitFor(() => expect(menu()?.textContent).toContain("Close conversation"));
  });
});

describe("the chat list's pinned chats, to assistive technology", () => {
  afterEach(() => {
    chat.clear();
    session.principal = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("says how to move a pin in words, and names the keys of a pinned row", () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    fakeNetwork(() => undefined);
    chat.rooms.reconcile([joinedRoom("jazz")]);
    const component = mount(ChatList, {
      target: document.body,
      props: { current: null, onjoin: () => {}, onnew: () => {} }
    });
    pins.list = [{ kind: "room", name: "jazz" }];
    flushSync();
    const header = document.querySelector('[data-line="h:pinned"]')!;
    expect(header.querySelector(".sr-only")?.textContent).toBe(
      "Move a pin with Alt+Shift+Up or Down"
    );
    expect(
      document.querySelector('[data-line="room:jazz"]')!.getAttribute("aria-keyshortcuts")
    ).toBe(keysAria(CHAT.movePin));
    unmount(component);
  });
});
