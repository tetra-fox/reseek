import { goto } from "$app/navigation";
import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { flushFaked } from "#lib/testing/flush.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Directory from "./Directory.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const summary = (username: string, fields: Partial<Schemas["UserSummary"]> = {}) =>
  ({
    username,
    known: true,
    isPrivileged: false,
    group: { name: "default", source: "builtin", priority: 1, color: null },
    tags: [],
    muted: false,
    pinned: false,
    lastSeenAt: "2026-10-26T10:00:00Z",
    figures: {
      downloads: { succeeded: 0, failed: 0, active: 0, queued: 0 },
      uploads: { succeeded: 0, failed: 0, active: 0, queued: 0 },
      messages: { in: 0, out: 0, unread: 0 }
    },
    unreadMessages: 0,
    activeTransfers: 0,
    ...fields
  }) as Schemas["UserSummary"];

describe("the users directory", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    session.principal = null;
    app.state = null;
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const start = async (users: Schemas["UserSummary"][]) => {
    vi.useFakeTimers();
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/users") ? { users, total: users.length } : undefined
    );
    live.start();
    component = mount(Directory, { target: document.body, props: {} });
    await flushFaked();
    await flushFaked();
    return net;
  };
  const grid = () => document.querySelector("[role=grid]")!;
  const line = (name: string) => document.querySelector(`[data-line="${name}"]`)!;

  it("lists pinned users first and patches a row in place", async () => {
    const net = await start([summary("bob"), summary("carol", { pinned: true })]);
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["carol", "bob"]);
    const bob = line("bob");
    net.frame("users.updated", summary("bob", { unreadMessages: 3 }));
    await flushFaked();
    expect(line("bob")).toBe(bob);
    expect(bob.textContent).toContain("3");
  });

  it("offers no buddy or block on our own row, nor in a selection of only it", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    await start([summary("me"), summary("bob")]);
    const labels = (name: string) =>
      [...line(name).querySelectorAll("button")].map((b) => b.ariaLabel);
    expect(labels("bob")).toEqual(expect.arrayContaining(["Add buddy", "Block"]));
    expect(labels("me")).not.toContain("Add buddy");
    expect(labels("me")).not.toContain("Block");
    line("me").querySelector<HTMLElement>("[role=checkbox]")!.click();
    await flushFaked();
    const bar = () => document.querySelector("[role=toolbar]")!;
    expect(bar().textContent).toContain("1 selected");
    expect(bar().textContent).not.toContain("Block");
  });

  it("moves, selects and opens with the list's keys", async () => {
    await start([summary("alice"), summary("bob"), summary("carol")]);
    press(grid(), "ArrowDown");
    press(grid(), "ArrowDown");
    expect(active(grid())).toBe("bob");
    press(grid(), " ");
    expect(ticked()).toEqual(["bob"]);
    expect(document.body.textContent).toContain("1 selected");
    press(grid(), "Enter");
    expect(goto).toHaveBeenCalledWith("#/users/bob");
    press(grid(), "a", { ctrlKey: true });
    expect(ticked()).toEqual(["alice", "bob", "carol"]);
    // a menu open takes the first Escape
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    document.body.append(menu);
    press(grid(), "Escape");
    expect(ticked()).toHaveLength(3);
    menu.remove();
    press(grid(), "Escape");
    expect(ticked()).toEqual([]);
    // a key with a modifier is the browser's
    expect(press(grid(), "ArrowDown", { altKey: true }).defaultPrevented).toBe(false);
  });
});
