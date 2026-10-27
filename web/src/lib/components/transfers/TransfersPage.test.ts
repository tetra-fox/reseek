import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { interaction } from "#lib/interaction.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { defaults, views } from "#lib/transfers/settings.svelte.js";
import type { Transfer } from "#lib/transfers/states.js";
import { downloads } from "#lib/transfers/transfers.svelte.js";

import TransfersPage from "./TransfersPage.svelte";

const flush = async () => {
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
};

/** the grouped listing the daemon answers, one directory per transfer */
const grouped = (rows: Transfer[]) =>
  rows.map((t) => ({
    username: t.username,
    directories: [{ directory: "d", fileCount: 1, files: [t] }]
  }));

describe("the transfers table", () => {
  let component: ReturnType<typeof mount> | null = null;
  let listing: Transfer[] = [];

  beforeEach(() => {
    vi.useFakeTimers();
    // jsdom lays nothing out, so the table's measured height stays at its first guess
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      }
    );
    Object.assign(views.download, defaults("download"), { groupBy: "flat" });
  });
  afterEach(() => {
    session.principal = null;
    if (component) unmount(component);
    live.stop();
    downloads.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const start = async () => {
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/transfers/downloads") ? grouped(listing) : undefined
    );
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
    component = mount(TransfersPage, { target: document.body, props: { direction: "download" } });
    await flush();
    await flush();
    return net;
  };
  const row = (id: string) => document.querySelector(`[role=row][data-id="${id}"]`);
  const order = () =>
    [...document.querySelectorAll("[role=rowgroup] [role=row]")]
      .toSorted(
        (a, b) =>
          parseFloat((a as HTMLElement).style.top) - parseFloat((b as HTMLElement).style.top)
      )
      .map((r) => r.getAttribute("data-id"));

  /** presses `key` on `target`, which bubbles to the table */
  const press = (target: Element, key: string, mods: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...mods });
    target.dispatchEvent(event);
    flushSync();
    return event;
  };
  const grid = () => document.querySelector("[role=grid]")!;
  const signIn = () =>
    (session.principal = {
      name: "a",
      role: "Administrator",
      session: null
    } as unknown as typeof session.principal);
  const ticked = () =>
    [...document.querySelectorAll('[role=row][aria-selected="true"]')].map((r) =>
      r.getAttribute("data-id")
    );
  const drawer = () => document.querySelector('aside[aria-label="Transfer details"]');

  it("patches a row in place by frame and by reconcile, and holds the order while frozen", async () => {
    const a = transfer({ filename: "x\\b.flac" });
    const b = transfer({ filename: "x\\c.flac" });
    listing = [a, b];
    const net = await start();
    expect(order()).toEqual([a.id, b.id]);
    const first = row(a.id)!;
    const name = first.querySelector("button")!;

    const selection = document.getSelection()!;
    selection.setBaseAndExtent(row(a.id)!, 0, row(b.id)!, row(b.id)!.childNodes.length);
    // the patched cells change their text, so what stays is where the selection starts and ends
    const held = () => {
      const now = document.getSelection()!;
      expect(now.isCollapsed).toBe(false);
      expect(now.anchorNode).toBe(row(a.id));
      expect(now.focusNode).toBe(row(b.id));
    };

    // a frame patches one row
    net.frame("transfers.updated", { ...a, state: "InProgress", percentComplete: 40 });
    await flush();
    expect(row(a.id)).toBe(first);
    expect(first.querySelector("button")).toBe(name);
    expect(first.textContent).toContain("40%");
    held();

    // a reconcile with a new row: the old rows keep their nodes
    const c = transfer({ filename: "x\\d.flac" });
    listing = [{ ...a, state: "InProgress", percentComplete: 55 }, b, c];
    await downloads.load();
    await flush();
    expect(row(a.id)).toBe(first);
    expect(first.textContent).toContain("55%");
    expect(order()).toEqual([a.id, b.id, c.id]);
    held();

    // while the user holds the table, a row that sorts first waits at the end
    const release = interaction.hold();
    const early = transfer({ filename: "x\\a.flac" });
    net.frame("transfers.added", early);
    await flush();
    expect(order()).toEqual([a.id, b.id, c.id, early.id]);
    expect(row(a.id)).toBe(first);
    release();
    await flush();
    // the selection inside the table holds it still too
    expect(order()).toEqual([a.id, b.id, c.id, early.id]);
    document.getSelection()!.removeAllRanges();
    await flush();
    expect(order()).toEqual([early.id, a.id, b.id, c.id]);
    expect(row(a.id)).toBe(first);
  });

  describe("keys", () => {
    it("leaves Ctrl, Cmd and Alt shortcuts to the browser on a focused row", async () => {
      session.principal = {
        name: "a",
        role: "Administrator",
        session: null
      } as unknown as typeof session.principal;
      const a = transfer({ state: "InProgress", percentComplete: 10 });
      listing = [a];
      const net = await start();
      const grid = document.querySelector("[role=grid]")!;
      row(a.id)!.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
      flushSync();
      const press = (key: string, mods: KeyboardEventInit = {}) => {
        const event = new KeyboardEvent("keydown", {
          key,
          bubbles: true,
          cancelable: true,
          ...mods
        });
        grid.dispatchEvent(event);
        return event;
      };
      const bulk = () => net.requests.filter((r) => r.url.pathname.endsWith("/bulk"));

      for (const mods of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
        expect(press("c", mods).defaultPrevented).toBe(false);
        expect(press("r", mods).defaultPrevented).toBe(false);
        expect(press("o", mods).defaultPrevented).toBe(false);
      }
      await flush();
      expect(bulk()).toEqual([]);

      // the plain key still cancels
      expect(press("c").defaultPrevented).toBe(true);
      await flush();
      expect(bulk().map((r) => r.body)).toEqual([{ action: "cancel", ids: [a.id] }]);
    });

    it("leaves Enter and Space on a username link to the link", async () => {
      const a = transfer();
      listing = [a];
      await start();
      const link = row(a.id)!.querySelector<HTMLAnchorElement>('a[href^="#/users/"]')!;
      link.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
      flushSync();
      const press = (key: string) => {
        const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
        link.dispatchEvent(event);
        return event;
      };

      expect(press("Enter").defaultPrevented).toBe(false);
      await flush();
      expect(document.querySelector('aside[aria-label="Transfer details"]')).toBeNull();

      press(" ");
      await flush();
      expect(document.querySelector('[aria-label="Selected transfers"]')).toBeNull();
    });

    it("act on the row of a focused checkbox, except Enter and Space", async () => {
      signIn();
      const a = transfer({ filename: "x\\a.flac", state: "Completed, Succeeded" });
      const b = transfer({ filename: "x\\b.flac", state: "Completed, Succeeded" });
      listing = [a, b];
      await start();
      const box = row(a.id)!.querySelector('[role="checkbox"]')!;
      box.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
      expect(press(box, "a", { ctrlKey: true }).defaultPrevented).toBe(true);
      expect(ticked()).toEqual([a.id, b.id]);
      expect(press(box, "Delete").defaultPrevented).toBe(true);
      await flush();
      expect(document.body.textContent).toContain("Remove 2 transfers?");
    });

    it("select a focused group header's transfers with Ctrl+A", async () => {
      Object.assign(views.download, { groupBy: "user" });
      const a = transfer({ username: "alice" });
      const b = transfer({ username: "bob" });
      listing = [a, b];
      await start();
      const header = row("user:bob")!.querySelector('[role="checkbox"]')!;
      header.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
      expect(press(header, "a", { metaKey: true }).defaultPrevented).toBe(true);
      expect(ticked()).toEqual([b.id]);
    });

    it("act on the row last clicked", async () => {
      const a = transfer({ filename: "x\\a.flac" });
      const b = transfer({ filename: "x\\b.flac" });
      listing = [a, b];
      await start();
      press(grid(), "ArrowDown");
      row(b.id)!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      press(grid(), " ");
      expect(ticked()).toEqual([b.id]);
      expect(grid().getAttribute("aria-activedescendant")).toBe(`transfer-${b.id}`);
    });

    it("do nothing on a row whose buttons don't offer it", async () => {
      signIn();
      const open = vi.spyOn(window, "open").mockReturnValue(null);
      const a = transfer({ state: "Queued, Remotely" });
      listing = [a];
      const net = await start();
      press(grid(), "ArrowDown");
      press(grid(), "r");
      press(grid(), "o");
      await flush();
      expect(net.requests.filter((r) => r.url.pathname.endsWith("/bulk"))).toEqual([]);
      expect(open).not.toHaveBeenCalled();
    });

    it("close the drawer with Escape before the selection, and a menu before the drawer", async () => {
      const a = transfer();
      listing = [a];
      await start();
      press(grid(), "ArrowDown");
      press(grid(), " ");
      press(grid(), "Enter");
      expect(drawer()).not.toBeNull();

      // a menu open over the drawer takes the Escape
      const menu = document.createElement("div");
      menu.setAttribute("role", "menu");
      document.body.append(menu);
      press(grid(), "Escape");
      expect(drawer()).not.toBeNull();
      expect(ticked()).toEqual([a.id]);
      menu.remove();

      press(grid(), "Escape");
      expect(drawer()).toBeNull();
      expect(ticked()).toEqual([a.id]);

      press(grid(), "Escape");
      expect(ticked()).toEqual([]);
    });
  });
});
