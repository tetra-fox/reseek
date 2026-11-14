import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { focusByKeys } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { box } from "#lib/testing/state.svelte.js";
import { type DirectoryAddress, directoryAt } from "#lib/users/directory.js";

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
    component = mount(Directory, {
      target: document.body,
      props: {
        address: directoryAt(new URLSearchParams()),
        onaddress: () => {}
      }
    });
    await flushFaked();
    await flushFaked();
    return net;
  };
  const line = (name: string) => document.querySelector(`[data-line="${name}"]`)!;

  it("lists buddies first and patches a row in place", async () => {
    const net = await start([
      userSummary("bob"),
      userSummary("carol", { buddy: { notifyPresence: false } })
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

  it("mounts a row's tooltips for a mouse, not a tap, so the button a tap focused takes its click", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    await start([userSummary("bob")]);
    const buddy = () => line("bob").querySelector<HTMLElement>('button[aria-label="Add buddy"]')!;
    const tapped = buddy();
    // a touch comes to the row, then the press that focuses the button
    line("bob").dispatchEvent(new PointerEvent("pointerenter", { pointerType: "touch" }));
    tapped.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    tapped.focus();
    await flushFaked();
    expect(buddy()).toBe(tapped);
    expect(tapped.hasAttribute("data-tooltip-trigger")).toBe(false);
    line("bob").dispatchEvent(new PointerEvent("pointerenter", { pointerType: "mouse" }));
    await flushFaked();
    expect(buddy().hasAttribute("data-tooltip-trigger")).toBe(true);
  });

  it("offers no live Unblock for a block the configuration made", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    const block = { username: "bob", reason: "", scopes: [], expiresAt: null, source: "config" };
    const net = await start([userSummary("bob", { block } as never), userSummary("carol")]);
    const unblock = () =>
      line("bob").querySelector<HTMLButtonElement>('button[aria-label="Unblock"]')!;
    // it keeps its focus, so the keys reach the reason in its tooltip, and does nothing. The keys
    // mount the tooltip of a row no pointer has come to, and the focus stays on the button
    focusByKeys(unblock());
    await flushFaked();
    expect(document.activeElement).toBe(unblock());
    expect(unblock().getAttribute("aria-disabled")).toBe("true");
    expect(unblock().disabled).toBe(false);
    expect(unblock().hasAttribute("data-tooltip-trigger")).toBe(true);
    unblock().click();
    await flushFaked();
    expect(net.requests.filter((r) => r.method !== "GET")).toEqual([]);
    line("bob").querySelector<HTMLElement>("[role=checkbox]")!.click();
    await flushFaked();
    const bar = () => document.querySelector("[role=toolbar]")!;
    expect(bar().textContent).toContain("1 selected");
    expect(bar().textContent).not.toContain("Unblock");
    line("carol").querySelector<HTMLElement>("[role=checkbox]")!.click();
    await flushFaked();
    expect(bar().textContent).toContain("Unblock");
  });

  describe("in pages", () => {
    const many = Array.from({ length: 120 }, (_, n) => userSummary(`user${n}`));
    /** the directory over `many`, a page at a time, answering a filter on Blocked with none */
    const paged = async (place: Partial<DirectoryAddress> = {}) => {
      vi.useFakeTimers();
      const net = fakeNetwork((url) => {
        if (!url.pathname.endsWith("/api/v0/users")) return undefined;
        const users = url.searchParams.get("blocked") ? many.slice(0, 3) : many;
        const offset = Number(url.searchParams.get("offset"));
        const limit = Number(url.searchParams.get("limit"));
        return { users: users.slice(offset, offset + limit), total: users.length };
      });
      const places: { page: number; size: number }[] = [];
      // the address, which what is written into it comes back from as the route does
      const address = box<DirectoryAddress>({ ...directoryAt(new URLSearchParams()), ...place });
      live.start();
      component = mount(Directory, {
        target: document.body,
        props: {
          get address() {
            return address.value;
          },
          onaddress: (next: DirectoryAddress) => {
            places.push({ page: next.page, size: next.size });
            address.value = next;
          }
        }
      });
      await flushFaked();
      await flushFaked();
      const asked = () =>
        net.requests
          .filter((r) => r.url.pathname.endsWith("/api/v0/users"))
          .map((r) => [r.url.searchParams.get("offset"), r.url.searchParams.get("limit")]);
      return { asked, places, address };
    };
    const lines = () => document.querySelectorAll("[data-line]").length;
    const pressed = (label: string) =>
      [...document.querySelectorAll<HTMLButtonElement>("button")].find(
        (b) => b.getAttribute("aria-label") === label || b.textContent?.trim() === label
      )!;

    it("reads only the page shown and keeps the place in the address", async () => {
      const { asked, places } = await paged();
      expect(lines()).toBe(50);
      pressed("Page 3").click();
      await flushFaked();
      expect(asked().at(-1)).toEqual(["100", "50"]);
      expect(lines()).toBe(20);
      expect(places).toEqual([{ page: 3, size: 50 }]);
      expect(pressed("Page 3").getAttribute("aria-current")).toBe("page");
      expect(pressed("Next page").disabled).toBe(true);
      // every row says where it is among all of them, the header first
      const rows = [...document.querySelectorAll('[role="grid"] [role="row"]')];
      expect(rows[0].getAttribute("aria-rowindex")).toBe("1");
      expect(rows[1].getAttribute("aria-rowindex")).toBe("102");
    });

    it("follows the address when a link moves it", async () => {
      const { asked, address } = await paged({ page: 3, size: 50 });
      expect(lines()).toBe(20);
      // the sidebar's link to the directory, with no page in it
      address.value = directoryAt(new URLSearchParams());
      await flushFaked();
      expect(asked().at(-1)).toEqual(["0", "50"]);
      expect(lines()).toBe(50);
      expect(pressed("Page 1").getAttribute("aria-current")).toBe("page");
    });

    it("starts again from the first page when a filter changes", async () => {
      const { asked, places } = await paged({ page: 2, size: 50 });
      expect(asked()).toEqual([["50", "50"]]);
      pressed("Blocked").click();
      await flushFaked();
      // one read, of the first page: the old page is never read under the new filter
      expect(asked()).toEqual([
        ["50", "50"],
        ["0", "50"]
      ]);
      expect(places).toEqual([{ page: 1, size: 50 }]);
    });

    it("keeps every filter in the address, so a link shows just what it names", async () => {
      const { address } = await paged({ page: 2, size: 50, hasTransfers: true });
      pressed("Blocked").click();
      await flushFaked();
      expect(address.value).toMatchObject({ hasTransfers: true, blocked: true, page: 1 });
      // the dashboard's count of users transferred with, followed later
      address.value = directoryAt(new URLSearchParams("has=transfers"));
      await flushFaked();
      expect(pressed("Blocked").getAttribute("aria-pressed")).toBe("false");
      expect(pressed("Has transfers").getAttribute("aria-pressed")).toBe("true");
    });

    it("lands on the last page when the one asked for is past the end", async () => {
      const { asked, places } = await paged({ page: 9, size: 25 });
      expect(places).toEqual([{ page: 5, size: 25 }]);
      expect(asked().at(-1)).toEqual(["100", "25"]);
      expect(lines()).toBe(20);
    });
  });
});
