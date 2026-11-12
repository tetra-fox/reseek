import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { chat } from "#lib/chat/chat.svelte.js";
import Username from "#lib/components/users/Username.svelte";
import { session } from "#lib/session.svelte.js";
import { joinedRoom } from "#lib/testing/fixtures.js";
import { press } from "#lib/testing/keys.js";
import { installMedia } from "#lib/testing/media.js";
import { fakeNetwork } from "#lib/testing/network.js";

import ChatPage from "./ChatPage.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn(), afterNavigate: vi.fn() }));

describe("the chat page", () => {
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

  it("moves between chats with Alt and the arrows on a narrow screen, the list closed", () => {
    installMedia();
    session.principal = { name: "me", role: "ReadWrite" } as never;
    fakeNetwork(() => undefined);
    chat.rooms.reconcile([
      joinedRoom("indie", { lastMessageAt: "2026-10-26T10:00:00Z" }),
      joinedRoom("jazz", { lastMessageAt: "2026-10-26T11:00:00Z" })
    ]);
    component = mount(ChatPage, {
      target: document.body,
      props: { target: { kind: "room", name: "jazz" }, at: null, q: "", kind: "all" }
    });
    flushSync();
    expect(document.querySelector('[aria-label="Rooms and conversations"]')).toBeNull();
    // with Shift it is the list's key for moving a pin
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", altKey: true, shiftKey: true })
    );
    expect(goto).not.toHaveBeenCalled();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", altKey: true }));
    expect(goto).toHaveBeenCalledWith("#/chat/rooms/indie", { reset: false });
  });

  it("leaves Alt+Down to a username, which shows its card, and stays on the chat", () => {
    installMedia();
    session.principal = { name: "me", role: "ReadWrite" } as never;
    fakeNetwork(() => undefined);
    chat.rooms.reconcile([
      joinedRoom("indie", { lastMessageAt: "2026-10-26T10:00:00Z" }),
      joinedRoom("jazz", { lastMessageAt: "2026-10-26T11:00:00Z" })
    ]);
    component = mount(ChatPage, {
      target: document.body,
      props: { target: { kind: "room", name: "jazz" }, at: null, q: "", kind: "all" }
    });
    const holder = document.body.appendChild(document.createElement("p"));
    const name = mount(Username, { target: holder, props: { name: "alice", card: true } });
    flushSync();
    const link = holder.querySelector("a")!;
    link.focus();
    press(link, "ArrowDown", { altKey: true });
    expect(goto).not.toHaveBeenCalled();
    expect(document.querySelector('[role="dialog"][aria-label="Profile of alice"]')).not.toBeNull();
    unmount(name);
  });
});
