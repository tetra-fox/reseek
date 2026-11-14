import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { chat } from "#lib/chat/chat.svelte.js";
import { joinedRoom } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import JoinRoomDialog from "./JoinRoomDialog.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const room = (name: string, userCount: number) => ({
  name,
  userCount,
  isPrivate: false,
  isOwned: false,
  isModerated: false,
  joined: false
});

describe("the join dialog", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    chat.clear();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const type = (selector: string, value: string) => {
    const field = document.querySelector<HTMLInputElement>(selector)!;
    field.value = value;
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  };
  const create = () => document.querySelector<HTMLButtonElement>('button[type="submit"]')!;

  it("lists the busiest rooms first and creates a private room by name", async () => {
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/rooms/available")) {
        return [room("jazz", 4), room("indie", 40), room("ambient", 4)];
      }
      if (method === "POST") {
        return { status: 403, body: { error: "A private room with this name already exists" } };
      }
      return undefined;
    });
    component = mount(JoinRoomDialog, { target: document.body, props: { open: true } });
    await flush();
    const names = () =>
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"));
    expect(names()).toEqual(["indie", "ambient", "jazz"]);
    // the grid draws a window of its rooms, so each row says where it is among them, the header
    // first
    expect(document.querySelector('[role="grid"]')!.getAttribute("aria-rowcount")).toBe("4");
    const rows = [...document.querySelectorAll('[role="grid"] [role="row"]')];
    expect(rows.map((r) => r.getAttribute("aria-rowindex"))).toEqual(["1", "2", "3", "4"]);

    type("#create-name", "club");
    document.querySelector<HTMLElement>("[role=switch]")!.click();
    flushSync();
    expect(create().textContent).toBe("Create");
    create().click();
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toEqual({
      name: "club",
      private: true
    });
    // the server's refusal shows in the dialog, which stays open
    expect(document.body.textContent).toContain("A private room with this name already exists");
  });

  it("offers a public room's Join instead of creating one by its name", async () => {
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/rooms/available") ? [room("indie", 40)] : undefined
    );
    component = mount(JoinRoomDialog, { target: document.body, props: { open: true } });
    await flush();
    type("#create-name", "indie");
    expect(create().disabled).toBe(true);
    expect(document.querySelector("[role=status]")!.textContent).toContain(
      "indie is already a public room"
    );
    create().closest("form")!.requestSubmit();
    await flush();
    expect(net.requests.some((r) => r.method === "POST")).toBe(false);
  });

  it("joins the first listed room on Enter in the filter", async () => {
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/rooms/available")) return [room("indie", 40), room("jazz", 4)];
      if (method === "POST") return { status: 403, body: { error: "no" } };
      return undefined;
    });
    component = mount(JoinRoomDialog, { target: document.body, props: { open: true } });
    await flush();
    type('input[aria-label="Filter rooms"]', "jaz");
    document
      .querySelector('input[aria-label="Filter rooms"]')!
      .dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })
      );
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toEqual({
      name: "jazz",
      private: false
    });
  });

  it("joins a room from its row in one click, the row then saying so", async () => {
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/rooms/available")) return [room("indie", 40)];
      if (url.pathname.endsWith("/rooms/joined") && method === "POST") {
        return { status: 201, body: { name: "indie", isPrivate: false, users: [], tickers: [] } };
      }
      if (url.pathname.endsWith("/rooms/joined")) {
        return [
          {
            name: "indie",
            unread: 0,
            mentions: 0,
            notify: "mentions",
            userCount: 40,
            lastMessageAt: null,
            lastMessage: null,
            lastReadId: 0,
            refused: false
          }
        ];
      }
      if (url.pathname.endsWith("/conversations")) return [];
      return undefined;
    });
    component = mount(JoinRoomDialog, { target: document.body, props: { open: true } });
    await flush();
    document.querySelector<HTMLButtonElement>('button[aria-label="Join indie"]')!.click();
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toEqual({
      name: "indie",
      private: false
    });
    expect(document.querySelector('[data-line="indie"]')!.textContent).toContain("Joined");
    expect(document.querySelector('button[aria-label="Leave indie"]')).not.toBeNull();
    // the dialog stays open for the next one
    expect(document.querySelector("[role=dialog]")).not.toBeNull();
  });

  it("offers Join on the row of a room the server refused after a login", async () => {
    chat.rooms.upsert(joinedRoom("indie", { refused: true }));
    const net = fakeNetwork((url, method) => {
      if (url.pathname.endsWith("/rooms/available")) return [room("indie", 40)];
      if (method === "POST") return { status: 403, body: { error: "no" } };
      return undefined;
    });
    component = mount(JoinRoomDialog, { target: document.body, props: { open: true } });
    await flush();
    expect(document.querySelector('[data-line="indie"]')!.textContent).not.toContain("Joined");
    document.querySelector<HTMLButtonElement>('button[aria-label="Join indie"]')!.click();
    await flush();
    expect(net.requests.find((r) => r.method === "POST")?.body).toEqual({
      name: "indie",
      private: false
    });
  });
});
