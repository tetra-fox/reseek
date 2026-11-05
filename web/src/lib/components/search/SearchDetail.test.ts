import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import InspectorPane from "#lib/components/inspector/InspectorPane.svelte";
import { toasts } from "#lib/feedback/toast.svelte.js";
import { UNDO_MS } from "#lib/feedback/undo.js";
import { inspectorPrefs } from "#lib/inspector/inspector.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { session } from "#lib/session.svelte.js";
import { searchOptions } from "#lib/settings.svelte.js";
import { flushFaked } from "#lib/testing/flush.js";
import { press } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import SearchDetail from "./SearchDetail.svelte";

const ID = "6f1c9a52-0d1e-4c43-9d55-3a1f0b6e7a10";

const file = (filename: string): Schemas["File"] => ({
  filename,
  size: 4_000_000,
  code: 1,
  extension: "flac",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

const response = (
  username: string,
  speed: number,
  files = [file(`@@${username}\\Album\\01 One.flac`), file(`@@${username}\\Album\\02 Two.flac`)]
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

const search = (fields: Partial<Schemas["Search"]>): Schemas["Search"] => ({
  id: ID,
  searchText: "album",
  token: 1,
  state: "InProgress",
  isComplete: false,
  startedAt: "2026-10-26T10:00:00Z",
  endedAt: null,
  responseCount: 0,
  fileCount: 0,
  lockedFileCount: 0,
  responseBytes: 0,
  responses: null,
  scope: { type: "network", subjects: [] },
  ...fields
});

describe("search results", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    if (component) unmount(component);
    inspectorPrefs.open = false;
    searchOptions.view = "cards";
    app.options = null;
    session.principal = null;
    toasts.splice(0);
    live.stop();
    searches.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("stream in without replacing the lines already shown, also once stored", async () => {
    const stored = [response("alice", 300), response("bob", 200), response("carol", 100)];
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/api/v0/users")) return { users: [], total: 0 };
      if (url.pathname.endsWith(`/searches/${ID}/responses`)) return stored;
      if (url.pathname.endsWith(`/searches/${ID}`)) return search({});
      return undefined;
    });
    searches.list.upsert(search({}));
    component = mount(SearchDetail, { target: document.body, props: { id: ID } });
    live.start();
    net.frame("hello", {
      principal: { name: "a", role: "ReadOnly" }
    } as unknown as Schemas["Hello"]);
    await flushFaked();

    net.frame("search.response", { searchId: ID, response: stored[0] });
    net.frame("search.response", { searchId: ID, response: stored[1] });
    await flushFaked();
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
    await flushFaked();
    expect(cards()).toHaveLength(3);
    expect(cards()[0]).toBe(alice);
    expect(cards()[1]).toBe(bob);
    expect(rowOf()).toBe(row);
    expect(document.getSelection()!.toString()).toBe(selected);

    // completed: the stored responses replace the streamed ones, and the lines stay
    net.frame(
      "search.update",
      search({
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

  /** Mounts the page on a running search and opens the stream; gives the fake network */
  const open = async (stored: Schemas["SearchResponse"][]) => {
    const net = fakeNetwork((url) => {
      if (url.pathname.endsWith("/api/v0/users")) return { users: [], total: 0 };
      if (url.pathname.endsWith(`/searches/${ID}/responses`)) return stored;
      if (url.pathname.endsWith(`/searches/${ID}`)) return search({});
      if (url.pathname.endsWith("/api/v0/options")) return { applied: [], pending: [] };
      return undefined;
    });
    searches.list.upsert(search({}));
    component = mount(SearchDetail, { target: document.body, props: { id: ID } });
    live.start();
    net.frame("hello", {
      principal: { name: "a", role: "ReadOnly" }
    } as unknown as Schemas["Hello"]);
    await flushFaked();
    return net;
  };

  it("keep the dense rows' nodes and the selection across them as results stream in", async () => {
    searchOptions.view = "dense";
    const stored = [response("alice", 300), response("bob", 200), response("carol", 100)];
    const net = await open(stored);
    net.frame("search.response", { searchId: ID, response: stored[0] });
    net.frame("search.response", { searchId: ID, response: stored[1] });
    await flushFaked();
    const grid = document.querySelector('[role=grid][aria-label^="Search results"]')!;
    const rows = () => [...grid.querySelectorAll("[role=row][data-line]")];
    expect(rows()).toHaveLength(4);
    expect(rows()[0].querySelectorAll(":scope > [role=gridcell]")).toHaveLength(11);
    const [first, , third] = rows();
    const selection = document.getSelection()!;
    selection.setBaseAndExtent(first, 0, third, third.childNodes.length);
    const selected = selection.toString();
    expect(selected).toContain("01 One.flac");

    net.frame("search.response", { searchId: ID, response: stored[2] });
    await flushFaked();
    expect(rows()).toHaveLength(6);
    expect(rows()[0]).toBe(first);
    expect(rows()[2]).toBe(third);
    expect(selection.toString()).toBe(selected);

    net.frame(
      "search.update",
      search({
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

  it("show no file in the inspector once the filter takes it out of the results", async () => {
    searchOptions.view = "dense";
    const stored = [response("alice", 300)];
    const net = await open(stored);
    const pane = mount(InspectorPane, { target: document.body });
    net.frame("search.response", { searchId: ID, response: stored[0] });
    await flushFaked();
    const grid = document.querySelector('[role=grid][aria-label^="Search results"]')!;
    const details = () => document.querySelector('aside[aria-label="Details"]');
    press(grid, "ArrowDown");
    press(grid, "i");
    expect(details()?.querySelector("h2")?.textContent).toBe("01 One.flac");

    const filter = document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!;
    filter.value = "nothing matches";
    filter.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(document.querySelector("[role=grid] [role=row][data-line]")).toBeNull();
    expect(details()?.querySelector("h2")).toBeNull();
    expect(details()?.textContent).toContain("Select a file to see its details.");
    unmount(pane);
  });

  it("open and close a folder, and with Alt every folder beside it", async () => {
    const two = response("alice", 1, [
      file("@@a\\Music\\One\\1.flac"),
      file("@@a\\Music\\Two\\2.flac"),
      file("@@a\\Music\\Three\\3.flac")
    ]);
    const net = await open([two]);
    net.frame("search.response", { searchId: ID, response: two });
    await flushFaked();
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
    await flushFaked();
    const text = () => document.body.textContent?.replace(/\s+/g, " ") ?? "";
    const button = (label: string) =>
      [...document.querySelectorAll("button")].filter((b) => b.textContent?.trim() === label);

    // a dismissed result is counted as dismissed, not as hidden by filters
    document.querySelector<HTMLButtonElement>('button[aria-label="Hide this result"]')!.click();
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
    const locked = { ...response("carol", 100, []), lockedFiles: [file("@@c\\x\\1.flac")] };
    const stored = [
      response("alice", 300),
      response("bob", 200),
      { ...locked, lockedFileCount: 1 }
    ];
    const net = await open(stored);
    for (const r of stored) net.frame("search.response", { searchId: ID, response: r });
    await flushFaked();
    const text = () => document.body.textContent?.replace(/\s+/g, " ") ?? "";
    const button = (label: string) =>
      [...document.querySelectorAll("button")].filter((b) => b.textContent?.trim() === label);
    // locked files are hidden from the start, which the line of filters does not name
    expect(searchOptions.hideLocked).toBe(true);
    expect(text()).not.toContain("Showing");
    expect(text()).toContain("1 result hidden by Locked files");

    // a dismissed user's files are not the filters' to count
    document.querySelector<HTMLButtonElement>('button[aria-label="Hide this result"]')!.click();
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

  it("put a preset in the filter box, which keeps only what it matches", async () => {
    app.options = {
      search: { filter_presets: [{ name: "FLAC", filter: "type:flac" }] }
    } as unknown as Schemas["OptionsDocument"];
    const mixed = response("alice", 1, [
      file("@@a\\x\\1.flac"),
      { ...file("@@a\\x\\2.mp3"), extension: "mp3" }
    ]);
    const net = await open([mixed]);
    net.frame("search.response", { searchId: ID, response: mixed });
    await flushFaked();
    const names = () =>
      [...document.querySelectorAll("[role=treegrid] [role=row][aria-selected]")].map(
        (r) => r.textContent?.match(/\d\.\w+/)?.[0]
      );
    expect(names()).toEqual(["1.flac", "2.mp3"]);
    const preset = [...document.querySelectorAll("button")].find((b) => b.textContent === "FLAC")!;
    preset.click();
    flushSync();
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(document.querySelector<HTMLInputElement>('input[aria-label="Filter"]')!.value).toBe(
      "type:flac"
    );
    expect(preset.getAttribute("aria-pressed")).toBe("true");
    expect(names()).toEqual(["1.flac"]);
  });

  it("keep every preset removed in a row off, and undo only the one undone", async () => {
    session.principal = { name: "a", role: "Administrator", session: null } as never;
    const presets = ["Big", "No small files", "FLAC"].map((name) => ({ name, filter: name }));
    app.options = {
      remote_configuration: true,
      search: { filter_presets: presets }
    } as unknown as Schemas["OptionsDocument"];
    const one = response("alice", 1);
    const net = await open([one]);
    net.frame("search.response", { searchId: ID, response: one });
    await flushFaked();
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
        }
      }
    ]);
    expect(chips()).toEqual(["No small files", "FLAC"]);
  });

  it("show a removed preset again once it is added back elsewhere", async () => {
    session.principal = { name: "a", role: "Administrator", session: null } as never;
    const big = { name: "Big", filter: "size:>100MB" };
    const flac = { name: "FLAC", filter: "type:flac" };
    const options = (presets: unknown[]) =>
      ({
        remote_configuration: true,
        search: { filter_presets: presets }
      }) as unknown as Schemas["OptionsDocument"];
    app.options = options([big, flac]);
    const one = response("alice", 1);
    const net = await open([one]);
    net.frame("search.response", { searchId: ID, response: one });
    await flushFaked();
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
