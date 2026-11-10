import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import InspectorPane from "#lib/components/inspector/InspectorPane.svelte";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { UNDO_MS } from "#lib/feedback/undo.js";
import { inspectorPrefs } from "#lib/inspector/inspector.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { resultList } from "#lib/search/list.js";
import { STREAM_MS } from "#lib/search/results.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { session } from "#lib/session.svelte.js";
import { searchOptions } from "#lib/settings.svelte.js";
import { search, sharedFile } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { press } from "#lib/testing/keys.js";
import { connected } from "#lib/testing/live.js";

import SearchDetail from "./SearchDetail.svelte";

const ID = "6f1c9a52-0d1e-4c43-9d55-3a1f0b6e7a10";

const response = (
  username: string,
  speed: number,
  files = [
    sharedFile(`@@${username}\\Album\\01 One.flac`),
    sharedFile(`@@${username}\\Album\\02 Two.flac`)
  ]
): Schemas["SearchResponse"] => ({
  username,
  country: null,
  token: 1,
  hasFreeUploadSlot: true,
  uploadSpeed: speed,
  queueLength: 0,
  fileCount: files.length,
  files,
  lockedFileCount: 0,
  lockedFiles: []
});

/** Lets the streamed responses be added to the results, then updates the page */
const streamed = async () => {
  await vi.advanceTimersByTimeAsync(STREAM_MS);
  flushSync();
};

describe("search results", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    if (component) unmount(component);
    inspectorPrefs.open = false;
    searchOptions.layout = "tree";
    resultList.sort = null;
    app.options = null;
    app.optionsRevision = null;
    session.principal = null;
    toasts.splice(0);
    live.stop();
    searches.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  /** Mounts the page on a running search and opens the stream; gives the fake network */
  const open = async (stored: Schemas["SearchResponse"][]) => {
    const net = connected((url) => {
      if (url.pathname.endsWith("/api/v0/users")) return { users: [], total: 0 };
      if (url.pathname.endsWith(`/searches/${ID}/responses`)) return stored;
      if (url.pathname.endsWith(`/searches/${ID}`)) return search({ id: ID });
      if (url.pathname.endsWith("/api/v0/options")) {
        return { applied: [], reconnectRequired: [], restartRequired: [], optionsRevision: "next" };
      }
      return undefined;
    });
    searches.list.upsert(search({ id: ID }));
    component = mount(SearchDetail, { target: document.body, props: { id: ID } });
    await flushFaked();
    return net;
  };

  it("stream in without replacing the lines already shown, also once stored", async () => {
    const stored = [response("alice", 300), response("bob", 200), response("carol", 100)];
    const net = await open(stored);
    net.frame("search.response", { searchId: ID, response: stored[0] });
    net.frame("search.response", { searchId: ID, response: stored[1] });
    await streamed();
    const cards = () => [
      ...document.querySelectorAll('[role=treegrid] [role=rowgroup] [data-line^="folder:user:"]')
    ];
    expect(cards().map((c) => c.getAttribute("data-line"))).toEqual([
      "folder:user:0:alice",
      "folder:user:0:bob"
    ]);
    const [alice, bob] = cards();
    const rowOf = () => document.querySelector("[role=treegrid] [role=row][aria-selected]")!;
    const row = rowOf();
    expect(row.textContent).toContain("01 One.flac");

    // a selection across the two users' lines
    const selection = document.getSelection()!;
    selection.setBaseAndExtent(alice, 0, bob, bob.childNodes.length);
    const selected = selection.toString();
    expect(selected).toContain("01 One.flac");

    net.frame("search.response", { searchId: ID, response: stored[2] });
    await streamed();
    expect(cards()).toHaveLength(3);
    expect(cards()[0]).toBe(alice);
    expect(cards()[1]).toBe(bob);
    expect(rowOf()).toBe(row);
    expect(document.getSelection()!.toString()).toBe(selected);

    // completed: the stored responses replace the streamed ones, and the lines stay
    net.frame(
      "search.update",
      search({
        id: ID,
        state: "Completed, TimedOut",
        isComplete: true,
        endedAt: "2026-10-26T10:00:15Z",
        responseCount: 3,
        fileCount: 6
      })
    );
    await flushFaked();
    await flushFaked();
    expect(net.requests.map((r) => r.url.pathname)).toContain(`/api/v0/searches/${ID}/responses`);
    expect(cards()).toHaveLength(3);
    expect(cards()[0]).toBe(alice);
    expect(cards()[1]).toBe(bob);
    expect(rowOf()).toBe(row);
    expect(document.getSelection()!.toString()).toBe(selected);
  });

  it("keep the list rows' nodes and the selection across them as results stream in", async () => {
    searchOptions.layout = "list";
    const stored = [response("alice", 300), response("bob", 200), response("carol", 100)];
    const net = await open(stored);
    net.frame("search.response", { searchId: ID, response: stored[0] });
    net.frame("search.response", { searchId: ID, response: stored[1] });
    await streamed();
    const grid = document.querySelector('[role=treegrid][aria-label^="Search results"]')!;
    const rows = () => [...grid.querySelectorAll("[role=row][data-line]")];
    expect(rows()).toHaveLength(4);
    // every column but the folder, which starts hidden
    expect(rows()[0].querySelectorAll(":scope > [role=gridcell]")).toHaveLength(10);
    const [first, , third] = rows();
    const selection = document.getSelection()!;
    selection.setBaseAndExtent(first, 0, third, third.childNodes.length);
    const selected = selection.toString();
    expect(selected).toContain("01 One.flac");

    net.frame("search.response", { searchId: ID, response: stored[2] });
    await streamed();
    expect(rows()).toHaveLength(6);
    expect(rows()[0]).toBe(first);
    expect(rows()[2]).toBe(third);
    expect(selection.toString()).toBe(selected);

    net.frame(
      "search.update",
      search({
        id: ID,
        state: "Completed, TimedOut",
        isComplete: true,
        endedAt: "2026-10-26T10:00:15Z",
        responseCount: 3
      })
    );
    await flushFaked();
    await flushFaked();
    expect(rows()[0]).toBe(first);
    expect(rows()[2]).toBe(third);
    expect(selection.toString()).toBe(selected);
  });

  it("open a list row's menu with a right-click, the lines holding still", async () => {
    searchOptions.layout = "list";
    // the fastest first, which aaron is
    resultList.sort = { id: "speed", dir: "desc" };
    const stored = [response("alice", 300), response("bob", 200), response("aaron", 400)];
    const net = await open(stored);
    net.frame("search.response", { searchId: ID, response: stored[0] });
    net.frame("search.response", { searchId: ID, response: stored[1] });
    await streamed();
    const grid = document.querySelector('[role=treegrid][aria-label^="Search results"]')!;
    const rows = () => [...grid.querySelectorAll("[role=row][data-line]")];
    const before = rows();
    const at = { bubbles: true, cancelable: true, clientX: 5, clientY: 5 };
    before[0].querySelector("[role=gridcell]")!.dispatchEvent(new MouseEvent("contextmenu", at));
    flushSync();
    await flushFaked();
    expect(document.querySelector("[role=menu]")).not.toBeNull();

    net.frame("search.response", { searchId: ID, response: stored[2] });
    await streamed();
    const keys = (lines: Element[]) => lines.map((l) => l.getAttribute("data-line"));
    expect(keys(rows()).slice(0, before.length)).toEqual(keys(before));
    expect(rows()[0]).toBe(before[0]);
  });

  it("show no file in the inspector once the filter takes it out of the results", async () => {
    searchOptions.layout = "list";
    const stored = [response("alice", 300)];
    const net = await open(stored);
    const pane = mount(InspectorPane, { target: document.body });
    net.frame("search.response", { searchId: ID, response: stored[0] });
    await streamed();
    const grid = document.querySelector('[role=treegrid][aria-label^="Search results"]')!;
    const details = () => document.querySelector('aside[aria-label="Details"]');
    press(grid, "ArrowDown");
    press(grid, "i");
    expect(details()?.querySelector("h2")?.textContent).toBe("01 One.flac");

    const filter = document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!;
    filter.value = "nothing matches";
    filter.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(document.querySelector("[role=treegrid] [role=row][data-line]")).toBeNull();
    expect(details()?.querySelector("h2")).toBeNull();
    expect(details()?.textContent).toContain("No file selected");
    unmount(pane);
  });

  it("keep the same files ticked from the list to the tree and back", async () => {
    searchOptions.layout = "list";
    const stored = [response("alice", 300), response("bob", 200)];
    const net = await open(stored);
    for (const r of stored) net.frame("search.response", { searchId: ID, response: r });
    await streamed();
    const rows = () => [...document.querySelectorAll("[role=treegrid] [role=row][data-line]")];
    // a row's key is its result's and its file's path, in both layouts
    const ticked = () =>
      rows()
        .filter((r) => r.getAttribute("aria-selected") === "true")
        .map((r) => r.getAttribute("data-line")?.split("\u0000")[1])
        .toSorted();
    const all = () =>
      document.querySelector('[aria-label="Select all"]')!.getAttribute("data-state");
    const tick = (row: Element) =>
      row.querySelector<HTMLButtonElement>('button[aria-label^="Select "]')!.click();

    tick(rows()[0]);
    tick(rows()[3]);
    flushSync();
    const picked = ["@@alice\\Album\\01 One.flac", "@@bob\\Album\\02 Two.flac"];
    expect(ticked()).toEqual(picked);
    expect(all()).toBe("indeterminate");

    searchOptions.layout = "tree";
    flushSync();
    await flushFaked();
    expect(ticked()).toEqual(picked);
    expect(all()).toBe("indeterminate");

    document.querySelector<HTMLButtonElement>('[aria-label="Select all"]')!.click();
    flushSync();
    expect(ticked()).toHaveLength(4);
    searchOptions.layout = "list";
    flushSync();
    await flushFaked();
    expect(ticked()).toHaveLength(4);
    expect(all()).toBe("checked");
  });

  it("open and close a folder, and with Alt every folder beside it", async () => {
    const two = response("alice", 1, [
      sharedFile("@@a\\Music\\One\\1.flac"),
      sharedFile("@@a\\Music\\Two\\2.flac"),
      sharedFile("@@a\\Music\\Three\\3.flac")
    ]);
    const net = await open([two]);
    net.frame("search.response", { searchId: ID, response: two });
    await streamed();
    const toggle = (name: string) =>
      document.querySelector<HTMLButtonElement>(`button[aria-label$=" ${name}"][aria-expanded]`)!;
    const files = () =>
      [...document.querySelectorAll("[role=treegrid] [role=row][aria-selected]")].map(
        (r) => r.textContent?.match(/\d\.flac/)?.[0]
      );
    expect(files()).toEqual(["1.flac", "2.flac", "3.flac"]);
    toggle("Two").click();
    await flushFaked();
    expect(files()).toEqual(["1.flac", "3.flac"]);
    expect(toggle("Two").getAttribute("aria-expanded")).toBe("false");
    // Alt on a closed one opens it and every sibling; on an open one closes them all
    toggle("Two").dispatchEvent(new MouseEvent("click", { bubbles: true, altKey: true }));
    await flushFaked();
    expect(files()).toEqual(["1.flac", "2.flac", "3.flac"]);
    toggle("One").dispatchEvent(new MouseEvent("click", { bubbles: true, altKey: true }));
    await flushFaked();
    expect(files()).toEqual([]);
    expect(toggle("Three").getAttribute("aria-expanded")).toBe("false");
  });

  it("count what is hidden by why, each with its own way back", async () => {
    const stored = [response("alice", 300), response("bob", 200)];
    const net = await open(stored);
    for (const r of stored) net.frame("search.response", { searchId: ID, response: r });
    await streamed();
    const text = () => document.body.textContent?.replace(/\s+/g, " ") ?? "";
    const button = (label: string) =>
      [...document.querySelectorAll("button")].filter((b) => b.textContent?.trim() === label);

    // a dismissed result is counted as dismissed, not as hidden by filters
    document.querySelector<HTMLButtonElement>('button[aria-label="Hide result"]')!.click();
    await flushFaked();
    expect(text()).toContain("1 dismissed");
    expect(text()).not.toContain("hidden by filters");

    // a filter that hides the rest leaves one message with one Clear filters, and no "shown"
    const box = document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!;
    box.value = "nothing-matches";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(text()).toContain("All results are hidden");
    expect(text()).toContain("1 result hidden by filters");
    expect(text()).not.toContain("All results shown");
    expect(button("Clear filters")).toHaveLength(1);

    // each way back brings back its own part
    button("Show again")[0].click();
    flushSync();
    expect(text()).not.toContain("dismissed");
    button("Clear filters")[0].click();
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(text()).not.toContain("hidden by filters");
  });

  it("count the filters' line out of what they act on, and leave the default toggle alone", async () => {
    const locked = { ...response("carol", 100, []), lockedFiles: [sharedFile("@@c\\x\\1.flac")] };
    const stored = [
      response("alice", 300),
      response("bob", 200),
      { ...locked, lockedFileCount: 1 }
    ];
    const net = await open(stored);
    for (const r of stored) net.frame("search.response", { searchId: ID, response: r });
    await streamed();
    const text = () => document.body.textContent?.replace(/\s+/g, " ") ?? "";
    const button = (label: string) =>
      [...document.querySelectorAll("button")].filter((b) => b.textContent?.trim() === label);
    // locked files are hidden from the start, which the line of filters does not name
    expect(searchOptions.hideLocked).toBe(true);
    expect(text()).not.toContain("Showing");
    expect(text()).toContain("1 result hidden by Locked files");

    // a dismissed user's files are not the filters' to count
    document.querySelector<HTMLButtonElement>('button[aria-label="Hide result"]')!.click();
    const box = document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!;
    box.value = "One";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(text()).toContain("Showing 1 of 2 files");
    expect(text()).not.toContain("Locked files hidden");

    button("Clear filters")[0].click();
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(text()).not.toContain("Showing");
    expect(searchOptions.hideLocked).toBe(true);
  });

  it("keep every preset removed in a row off, and undo only the one undone", async () => {
    session.principal = { name: "a", role: "Administrator", session: null } as never;
    const presets = ["Big", "No small files", "FLAC"].map((name) => ({ name, filter: name }));
    app.options = {
      remote_configuration: true,
      search: { filter_presets: presets }
    } as unknown as Schemas["OptionsDocument"]["values"];
    app.optionsRevision = "read";
    const one = response("alice", 1);
    const net = await open([one]);
    net.frame("search.response", { searchId: ID, response: one });
    await streamed();
    const chips = () =>
      [...document.querySelectorAll('[aria-label="Filter presets"] button[aria-pressed]')].map(
        (b) => b.textContent?.trim()
      );
    expect(chips()).toEqual(["Big", "No small files", "FLAC"]);
    const remove = (name: string) =>
      document.querySelector<HTMLButtonElement>(`button[aria-label="Remove preset ${name}"]`)!;
    remove("Big").click();
    await flushFaked();
    remove("No small files").click();
    await flushFaked();
    expect(chips()).toEqual(["FLAC"]);
    // Undo on the second puts back the second alone
    toasts.find((t) => t.text.includes("No small files"))!.action!.run();
    await flushFaked();
    expect(chips()).toEqual(["No small files", "FLAC"]);
    await vi.advanceTimersByTimeAsync(UNDO_MS);
    flushSync();
    const saved = net.requests.filter((r) => r.method === "PATCH").map((r) => r.body);
    expect(saved).toEqual([
      {
        set: {
          "search.filter_presets": [
            { name: "No small files", filter: "No small files" },
            { name: "FLAC", filter: "FLAC" }
          ]
        },
        unset: [],
        expiresIn: null,
        revision: "read"
      }
    ]);
    expect(chips()).toEqual(["No small files", "FLAC"]);
    expect(app.optionsRevision, "the tab takes what it wrote").toBe("next");
  });

  it("show a removed preset again once it is added back elsewhere", async () => {
    session.principal = { name: "a", role: "Administrator", session: null } as never;
    const big = { name: "Big", filter: "size:>100MB" };
    const flac = { name: "FLAC", filter: "type:flac" };
    const options = (presets: unknown[]) =>
      ({
        remote_configuration: true,
        search: { filter_presets: presets }
      }) as unknown as Schemas["OptionsDocument"]["values"];
    app.options = options([big, flac]);
    const one = response("alice", 1);
    const net = await open([one]);
    net.frame("search.response", { searchId: ID, response: one });
    await streamed();
    const chips = () =>
      [...document.querySelectorAll('[aria-label="Filter presets"] button[aria-pressed]')].map(
        (b) => b.textContent?.trim()
      );
    document.querySelector<HTMLButtonElement>('button[aria-label="Remove preset Big"]')!.click();
    await vi.advanceTimersByTimeAsync(UNDO_MS);
    flushSync();
    expect(chips()).toEqual(["FLAC"]);
    // the options come back without it, then someone adds it again from another tab
    app.options = options([flac]);
    flushSync();
    app.options = options([flac, big]);
    flushSync();
    expect(chips()).toEqual(["FLAC", "Big"]);
  });
});
