import { type ComponentProps, flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import InspectorPane from "#lib/components/inspector/InspectorPane.svelte";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { UNDO_MS } from "#lib/feedback/undo.js";
import { inspectorPrefs, SETTLE_MS } from "#lib/inspector/inspector.svelte.js";
import { interaction } from "#lib/lists/interaction.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { press, ticked } from "#lib/testing/keys.js";
import { connected } from "#lib/testing/live.js";
import { defaults, views } from "#lib/transfers/grid/settings.svelte.js";
import type { Transfer } from "#lib/transfers/states.js";
import { downloads } from "#lib/transfers/transfers.svelte.js";

import TransfersPage from "./TransfersPage.svelte";

vi.mock("$app/state", () => ({ page: { url: new URL("http://localhost/#/downloads") } }));

/** the grouped listing the daemon answers, one directory per transfer */
const grouped = (rows: Transfer[]) =>
  rows.map((t) => ({
    username: t.username,
    directories: [{ directory: "d", fileCount: 1, files: [t] }]
  }));

describe("the transfers table", () => {
  let component: ReturnType<typeof mount> | null = null;
  let hostPane: ReturnType<typeof mount> | null = null;
  let listing: Transfer[] = [];

  beforeEach(() => {
    vi.useFakeTimers();
    Object.assign(views.download, defaults("download"), { layout: "list" });
  });
  afterEach(() => {
    session.principal = null;
    if (component) unmount(component);
    if (hostPane) unmount(hostPane);
    inspectorPrefs.open = false;
    live.stop();
    downloads.clear();
    toasts.splice(0);
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  const start = async (props: ComponentProps<typeof TransfersPage> = { direction: "download" }) => {
    const net = connected((url) =>
      url.pathname.endsWith("/api/v0/transfers/downloads") ? grouped(listing) : undefined
    );
    component = mount(TransfersPage, { target: document.body, props });
    hostPane = mount(InspectorPane, { target: document.body });
    await flushFaked();
    await flushFaked();
    return net;
  };
  const row = (id: string) =>
    document.querySelector(`[role=rowgroup] [role=row][data-line="${id}"]`);
  const order = () =>
    [...document.querySelectorAll("[role=rowgroup] [role=row]")]
      .toSorted(
        (a, b) =>
          parseFloat((a as HTMLElement).style.top) - parseFloat((b as HTMLElement).style.top)
      )
      .map((r) => r.getAttribute("data-line"));

  const grid = () => document.querySelector("[role=treegrid]")!;
  const signIn = () =>
    (session.principal = {
      name: "a",
      role: "Administrator",
      session: null
    } as unknown as typeof session.principal);
  const pane = () => document.querySelector('aside[aria-label="Details"]');
  /** Lets the inspector's waits run out and its answers come back */
  const settle = async () => {
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    flushSync();
  };

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
    await flushFaked();
    expect(row(a.id)).toBe(first);
    expect(first.querySelector("button")).toBe(name);
    expect(first.textContent).toContain("40%");
    held();

    // a reconcile with a new row: the old rows keep their nodes
    const c = transfer({ filename: "x\\d.flac" });
    listing = [{ ...a, state: "InProgress", percentComplete: 55 }, b, c];
    await downloads.load();
    await flushFaked();
    expect(row(a.id)).toBe(first);
    expect(first.textContent).toContain("55%");
    expect(order()).toEqual([a.id, b.id, c.id]);
    held();

    // while the user holds the table, a row that sorts first waits at the end
    const release = interaction.hold();
    const early = transfer({ filename: "x\\a.flac" });
    net.frame("transfers.added", early);
    await flushFaked();
    expect(order()).toEqual([a.id, b.id, c.id, early.id]);
    expect(row(a.id)).toBe(first);
    release();
    await flushFaked();
    // the selection inside the table holds it still too
    expect(order()).toEqual([a.id, b.id, c.id, early.id]);
    document.getSelection()!.removeAllRanges();
    await flushFaked();
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
      await flushFaked();
      expect(bulk(net)).toEqual([]);

      // the plain key still cancels
      expect(press(grid(), "c").defaultPrevented).toBe(true);
      await flushFaked();
      expect(bulk(net).map((r) => r.body)).toEqual([{ action: "cancel", ids: [a.id] }]);
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
      await flushFaked();
      expect(pane()).toBeNull();
      press(box, " ");
      expect(ticked()).toEqual([a.id]);

      expect(press(box, "a", { ctrlKey: true }).defaultPrevented).toBe(true);
      expect(ticked()).toEqual([a.id, b.id]);
      expect(press(box, "Delete").defaultPrevented).toBe(true);
      await flushFaked();
      // removed from the screen at once, from the daemon once the undo has passed
      expect(toasts[0].text).toBe("Removed 2 transfers");
      expect([row(a.id), row(b.id)]).toEqual([null, null]);
    });

    it("remove the focused row alone when it is outside the selection, and undo it", async () => {
      signIn();
      const a = transfer({ filename: "x\\a.flac", state: "Completed, Succeeded" });
      const b = transfer({ filename: "x\\b.flac", state: "Completed, Succeeded" });
      const c = transfer({ filename: "x\\c.flac", state: "Completed, Succeeded" });
      listing = [a, b, c];
      const net = await start();
      press(grid(), "ArrowDown");
      press(grid(), " ", { shiftKey: true });
      press(grid(), "ArrowDown");
      press(grid(), " ", { shiftKey: true });
      press(grid(), "ArrowDown");
      expect(ticked()).toEqual([a.id, b.id]);
      press(grid(), "Delete");
      await flushFaked();
      expect(row(c.id)).toBeNull();
      expect(row(a.id)).not.toBeNull();
      // Undo brings the row back and nothing is asked of the daemon
      toasts[0].action!.run();
      await vi.advanceTimersByTimeAsync(UNDO_MS);
      flushSync();
      expect(row(c.id)).not.toBeNull();
      expect(bulk(net)).toEqual([]);
    });

    it("select a group from its header with Ctrl+A and Space, and close it with Enter", async () => {
      signIn();
      Object.assign(views.download, { layout: "tree", groupBy: "user" });
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
      for (const key of ["Delete", "r", "c", "o"]) {
        expect(press(grid(), key).defaultPrevented).toBe(false);
      }
      await flushFaked();
      expect(bulk(net)).toEqual([]);
      expect(pane()).toBeNull();
      // a header is a folder line: Enter closes its group
      press(grid(), "Enter");
      expect(row(b.id)).toBeNull();
    });

    it("select only the group's transfers still listed", async () => {
      const a = transfer({ filename: "x\\a.flac" });
      const b = transfer({ filename: "x\\b.mp3" });
      listing = [a, b];
      Object.assign(views.download, { layout: "tree", groupBy: "user" });
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
      press(grid(), " ", { shiftKey: true });
      expect(ticked()).toEqual([b.id]);
      const active = grid().getAttribute("aria-activedescendant")!;
      expect(document.getElementById(active)?.getAttribute("data-line")).toBe(b.id);
    });

    it("move past a folded group to the next row", async () => {
      Object.assign(views.download, { layout: "tree", groupBy: "user" });
      const a = transfer({ username: "alice" });
      const b = transfer({ username: "bob" });
      const c = transfer({ username: "carol" });
      listing = [a, b, c];
      await start();
      const fold = row("user:bob")!.querySelector('button[aria-label="Close bob"]')!;
      fold.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      (fold as HTMLButtonElement).click();
      flushSync();
      expect(row(b.id)).toBeNull();
      // carol's header, then her row
      press(grid(), "ArrowDown");
      press(grid(), "ArrowDown");
      press(grid(), " ", { shiftKey: true });
      expect(ticked()).toEqual([c.id]);
      // her header, bob's folded one, then alice's row
      press(grid(), "ArrowUp");
      press(grid(), "ArrowUp");
      press(grid(), "ArrowUp");
      press(grid(), " ", { shiftKey: true });
      expect(ticked().toSorted()).toEqual([a.id, c.id].toSorted());
    });

    it("keep focus on a row control while its row is drawn, and on the table once it is not", async () => {
      listing = Array.from({ length: 80 }, (_, n) =>
        transfer({ filename: `x\\${String(n).padStart(2, "0")}.flac` })
      );
      await start();
      const scrollTo = (top: number) => {
        Object.defineProperty(grid(), "scrollTop", { value: top, configurable: true });
        grid().dispatchEvent(new Event("scroll"));
        flushSync();
      };
      const box = row(listing[5].id)!.querySelector<HTMLElement>('[role="checkbox"]')!;
      box.focus();
      // three rows down, the sixth row is still drawn
      scrollTo(36 * 3);
      expect(document.activeElement).toBe(box);
      scrollTo(2000);
      expect(document.activeElement).toBe(grid());
      expect(grid().getAttribute("aria-activedescendant")).toBeNull();
    });

    it("give focus to the table when a confirm closes onto a control that left", async () => {
      signIn();
      Object.assign(views.download, { layout: "tree", groupBy: "user" });
      const a = transfer({ filename: "x\\a.flac", state: "InProgress", percentComplete: 5 });
      const b = transfer({ filename: "x\\b.flac", state: "InProgress", percentComplete: 5 });
      const other = transfer({ username: "bob", filename: "y\\c.flac" });
      listing = [a, b, other];
      const net = await start();
      const cancelGroup = row("user:alice")!.querySelector<HTMLButtonElement>(
        'button[aria-label="Cancel the group"]'
      )!;
      cancelGroup.focus();
      // a group with running transfers asks before it is canceled
      cancelGroup.click();
      await flushFaked();
      expect(document.body.textContent).toContain("Cancel 2 active transfers?");
      const dismiss = [...document.querySelectorAll("button")].find(
        (x) => x.textContent === "Cancel"
      )!;
      for (const t of [a, b]) {
        net.frame("transfers.removed", { id: t.id, username: t.username, direction: "Download" });
      }
      await flushFaked();
      dismiss.click();
      await vi.advanceTimersByTimeAsync(500);
      flushSync();
      expect(document.activeElement).toBe(grid());
    });

    it("select a group from the pinned header after scrolling", async () => {
      Object.assign(views.download, { layout: "tree", groupBy: "user" });
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
        '[data-pinned][data-line="user:alice"] [role="checkbox"]'
      )!;
      pinned.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      press(pinned, "a", { ctrlKey: true });
      expect(ticked().length).toBeGreaterThan(0);
      expect(document.body.textContent).toContain("25 selected");
    });

    it("show no transfer in the inspector once the filter takes it out of the table", async () => {
      const a = transfer({ filename: "x\\a.flac" });
      listing = [a];
      await start();
      press(grid(), "ArrowDown");
      press(grid(), "i");
      expect(pane()?.querySelector("h2")?.textContent).toBe("a.flac");
      const filter = document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!;
      filter.value = "nothing matches";
      filter.dispatchEvent(new Event("input", { bubbles: true }));
      await vi.advanceTimersByTimeAsync(200);
      flushSync();
      expect(row(a.id)).toBeNull();
      expect(pane()?.querySelector("h2")).toBeNull();
      expect(pane()?.textContent).toContain("No file selected");
    });

    it("give focus back to the table when the inspector closes from inside", async () => {
      const a = transfer();
      listing = [a];
      await start();
      press(grid(), "ArrowDown");
      press(grid(), "i");
      const close = pane()!.querySelector<HTMLButtonElement>('[aria-label="Close details"]')!;
      close.focus();
      close.click();
      await vi.runOnlyPendingTimersAsync();
      flushSync();
      expect(pane()).toBeNull();
      expect(document.activeElement).toBe(grid());
    });

    it("give focus to the pane when its menu removes the last transfer", async () => {
      const a = transfer();
      listing = [a];
      await start();
      signIn();
      press(grid(), "ArrowDown");
      press(grid(), "i");
      const more = pane()!.querySelector<HTMLButtonElement>('[aria-label="More actions"]')!;
      more.focus();
      press(more, "Enter");
      await vi.runOnlyPendingTimersAsync();
      flushSync();
      const remove = [...document.querySelectorAll<HTMLElement>("[role=menuitem]")].find((i) =>
        i.textContent?.includes("Remove")
      )!;
      remove.focus();
      press(remove, "Enter");
      // short of the Undo's countdown, which would carry the removal out
      await vi.advanceTimersByTimeAsync(100);
      flushSync();
      expect(pane()?.textContent).toContain("No file selected");
      expect(document.activeElement).toBe(pane());
    });

    it("leave the docked inspector open on Escape, a menu taking it first", async () => {
      const a = transfer();
      listing = [a];
      await start();
      press(grid(), "ArrowDown");
      press(grid(), " ", { shiftKey: true });
      press(grid(), "Enter");
      expect(pane()).not.toBeNull();

      const menu = document.createElement("div");
      menu.setAttribute("role", "menu");
      document.body.append(menu);
      press(grid(), "Escape");
      expect(ticked()).toEqual([a.id]);
      menu.remove();

      press(grid(), "Escape");
      expect(ticked()).toEqual([]);
      expect(pane()).not.toBeNull();
    });
  });

  it("asks for a transfer's history again when it moves to a new state", async () => {
    const a = transfer({ state: "Queued, Remotely" });
    listing = [a];
    const net = await start();
    press(grid(), "ArrowDown");
    press(grid(), "Enter");
    await settle();
    const asks = () =>
      net.requests.filter((r) => r.url.searchParams.get("includeHistory") === "true").length;
    expect(asks()).toBe(1);
    net.frame("transfers.updated", { ...a, percentComplete: 0 });
    await settle();
    expect(asks()).toBe(1);
    net.frame("transfers.updated", { ...a, state: "InProgress" });
    await settle();
    expect(asks()).toBe(2);
  });

  it("asks about the file focus rests on, not those it passes", async () => {
    const files = ["a", "b", "c"].map((n) => transfer({ filename: `x\\${n}.flac` }));
    listing = files;
    const net = await start();
    press(grid(), "ArrowDown");
    press(grid(), "Enter");
    const histories = () =>
      net.requests.filter((r) => r.url.searchParams.get("includeHistory") === "true");
    press(grid(), "ArrowDown");
    await vi.advanceTimersByTimeAsync(SETTLE_MS / 2);
    press(grid(), "ArrowDown");
    await settle();
    expect(histories().map((r) => r.url.pathname.split("/").at(-1))).toEqual([files[2].id]);
  });
  it("shows one download job's files when the Jobs page links to it", async () => {
    listing = [
      transfer({ id: "a", jobId: "J1" }),
      transfer({ id: "b", jobId: "J2", filename: "@@a\\Music\\Album\\02 Two.flac" })
    ];
    await start({ direction: "download", job: "J1" });
    expect(row("a")).not.toBeNull();
    expect(row("b")).toBeNull();
    expect(document.body.textContent).toContain("One download job");
  });
});
