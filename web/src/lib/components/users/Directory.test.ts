import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Directory from "./Directory.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

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
  const line = (name: string) => document.querySelector(`[data-line="${name}"]`)!;

  it("lists buddies first and patches a row in place", async () => {
    const net = await start([
      userSummary("bob"),
      userSummary("carol", { buddy: { trusted: false, notifyPresence: false } })
    ]);
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["carol", "bob"]);
    const bob = line("bob");
    net.frame("users.updated", userSummary("bob", { unreadMessages: 3 }));
    await flushFaked();
    expect(line("bob")).toBe(bob);
    expect(bob.textContent).toContain("3");
  });

  it("offers no buddy or block on our own row, nor in a selection of only it", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    await start([userSummary("me"), userSummary("bob")]);
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

  it("offers no live Unblock for a block the configuration made", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    const block = { username: "bob", reason: "", scopes: [], expiresAt: null, source: "config" };
    await start([userSummary("bob", { block } as never), userSummary("carol")]);
    const unblock = line("bob").querySelector<HTMLButtonElement>('button[aria-label="Unblock"]')!;
    expect(unblock.disabled).toBe(true);
    // the reason is the tooltip of the span around it, which the keys can reach
    expect(unblock.parentElement!.hasAttribute("data-tooltip-trigger")).toBe(true);
    expect(unblock.parentElement!.tabIndex).toBe(0);
    line("bob").querySelector<HTMLElement>("[role=checkbox]")!.click();
    await flushFaked();
    const bar = () => document.querySelector("[role=toolbar]")!;
    expect(bar().textContent).toContain("1 selected");
    expect(bar().textContent).not.toContain("Unblock");
    line("carol").querySelector<HTMLElement>("[role=checkbox]")!.click();
    await flushFaked();
    expect(bar().textContent).toContain("Unblock");
  });
});
