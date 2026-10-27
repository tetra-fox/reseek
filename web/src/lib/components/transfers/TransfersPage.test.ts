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
    const bulk = (net: Awaited<ReturnType<typeof start>>) =>
      net.requests.filter((r) => r.url.pathname.endsWith("/bulk"));
    const focusIn = (target: Element) =>
      target.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));

    it("leave Ctrl, Cmd and Alt shortcuts to the browser on a focused row", async () => {
      signIn();
      const a = transfer({ state: "InProgress", percentComplete: 10 });
      listing = [a];
      const net = await start();
      press(grid(), "ArrowDown");
      for (const mods of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
        for (const key of ["c", "r", "o"])
          expect(press(grid(), key, mods).defaultPrevented).toBe(false);
      }
      await flush();
      expect(bulk(net)).toEqual([]);

      // the plain key still cancels
      expect(press(grid(), "c").defaultPrevented).toBe(true);
      await flush();
      expect(bulk(net).map((r) => r.body)).toEqual([{ action: "cancel", ids: [a.id] }]);
    });

    it("leave Enter and Space on a username link to the link", async () => {
      const a = transfer();
      listing = [a];
      await start();
      const link = row(a.id)!.querySelector('a[href^="#/users/"]')!;
      focusIn(link);
      expect(press(link, "Enter").defaultPrevented).toBe(false);
      await flush();
      expect(drawer()).toBeNull();
      press(link, " ");
      expect(ticked()).toEqual([]);
    });

    it("leave Enter and Space to a focused checkbox and act on its row with the rest", async () => {
      signIn();
      const a = transfer({ filename: "x\\a.flac", state: "Completed, Succeeded" });
      const b = transfer({ filename: "x\\b.flac", state: "Completed, Succeeded" });
      listing = [a, b];
      await start();
      const box = row(a.id)!.querySelector('[role="checkbox"]')!;
      focusIn(box);
      // the checkbox takes both: Enter does nothing, Space ticks its row
      press(box, "Enter");
      await flush();
      expect(drawer()).toBeNull();
      press(box, " ");
      expect(ticked()).toEqual([a.id]);

      expect(press(box, "a", { ctrlKey: true }).defaultPrevented).toBe(true);
      expect(ticked()).toEqual([a.id, b.id]);
      expect(press(box, "Delete").defaultPrevented).toBe(true);
      await flush();
      expect(document.body.textContent).toContain("Remove 2 transfers?");
    });

    it("select a group from its header with Ctrl+A and Space, and nothing else", async () => {
      signIn();
      Object.assign(views.download, { groupBy: "user" });
      const a = transfer({ username: "alice" });
      const b = transfer({ username: "bob" });
      listing = [a, b];
      const net = await start();
      const header = row("user:bob")!.querySelector('[role="checkbox"]')!;
      focusIn(header);
      expect(press(header, "a", { metaKey: true }).defaultPrevented).toBe(true);
      expect(ticked()).toEqual([b.id]);
      press(grid(), " ");
      expect(ticked()).toEqual([]);
      press(grid(), " ");
      expect(ticked()).toEqual([b.id]);
      for (const key of ["Delete", "r", "c", "o", "Enter"]) {
        expect(press(grid(), key).defaultPrevented).toBe(false);
      }
      await flush();
      expect(bulk(net)).toEqual([]);
      expect(drawer()).toBeNull();
    });

    it("select only the group's transfers still listed", async () => {
      const a = transfer({ filename: "x\\a.flac" });
      const b = transfer({ filename: "x\\b.mp3" });
      listing = [a, b];
      Object.assign(views.download, { groupBy: "user" });
      await start();
      row(a.id)!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      const filter = document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!;
      filter.value = "ext:flac";
      filter.dispatchEvent(new Event("input", { bubbles: true }));
      await vi.advanceTimersByTimeAsync(200);
      flushSync();
      expect(row(b.id)).toBeNull();
      press(grid(), "a", { ctrlKey: true });
      expect(ticked()).toEqual([a.id]);
      expect(document.body.textContent).toContain("1 selected");
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
      const active = grid().getAttribute("aria-activedescendant")!;
      expect(document.getElementById(active)?.getAttribute("data-id")).toBe(b.id);
    });

    it("move past a folded group to the next row", async () => {
      Object.assign(views.download, { groupBy: "user" });
      const a = transfer({ username: "alice" });
      const b = transfer({ username: "bob" });
      const c = transfer({ username: "carol" });
      listing = [a, b, c];
      await start();
      const fold = row("user:bob")!.querySelector('button[aria-label="Fold"]')!;
      fold.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      (fold as HTMLButtonElement).click();
      flushSync();
      expect(row(b.id)).toBeNull();
      press(grid(), "ArrowDown");
      press(grid(), " ");
      expect(ticked()).toEqual([c.id]);
      press(grid(), "ArrowUp");
      press(grid(), " ");
      expect(ticked().toSorted()).toEqual([a.id, c.id].toSorted());
    });

    it("go on from the row that took a removed row's place", async () => {
      signIn();
      const a = transfer({ filename: "x\\a.flac", state: "Completed, Succeeded" });
      const b = transfer({ filename: "x\\b.flac", state: "Completed, Succeeded" });
      const c = transfer({ filename: "x\\c.flac", state: "Completed, Succeeded" });
      listing = [a, b, c];
      const net = await start();
      press(grid(), "ArrowDown");
      press(grid(), "ArrowDown");
      press(grid(), "Delete");
      await flush();
      expect(bulk(net).map((r) => r.body)).toEqual([{ action: "remove", ids: [b.id] }]);
      net.frame("transfers.removed", { id: b.id, username: b.username, direction: "Download" });
      await flush();
      expect(row(b.id)).toBeNull();
      // c took b's place, so ↓ has nowhere further to go and ↑ reaches a
      press(grid(), " ");
      expect(ticked()).toEqual([c.id]);
      press(grid(), "ArrowUp");
      press(grid(), " ");
      expect(ticked().toSorted()).toEqual([a.id, c.id].toSorted());
    });

    it("keep focus on the table when a focused row control scrolls away", async () => {
      listing = Array.from({ length: 80 }, (_, n) =>
        transfer({ filename: `x\\${String(n).padStart(2, "0")}.flac` })
      );
      await start();
      const box = row(listing[0].id)!.querySelector<HTMLElement>('[role="checkbox"]')!;
      box.focus();
      Object.defineProperty(grid(), "scrollTop", { value: 2000, configurable: true });
      grid().dispatchEvent(new Event("scroll"));
      flushSync();
      expect(document.activeElement).toBe(grid());
      expect(grid().getAttribute("aria-activedescendant")).toBeNull();
    });

    it("select a group from the pinned header after scrolling", async () => {
      Object.assign(views.download, { groupBy: "user" });
      listing = Array.from({ length: 30 }, (_, n) =>
        transfer({
          username: n < 25 ? "alice" : "bob",
          filename: `x\\${String(n).padStart(2, "0")}.flac`
        })
      );
      await start();
      Object.defineProperty(grid(), "scrollTop", { value: 36 * 5, configurable: true });
      grid().dispatchEvent(new Event("scroll"));
      flushSync();
      const pinned = document.querySelector(
        '[data-line="user:alice"]:not([role=row]) [role="checkbox"]'
      )!;
      pinned.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      press(pinned, "a", { ctrlKey: true });
      expect(ticked().length).toBeGreaterThan(0);
      expect(document.body.textContent).toContain("25 selected");
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
      expect(bulk(net)).toEqual([]);
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
