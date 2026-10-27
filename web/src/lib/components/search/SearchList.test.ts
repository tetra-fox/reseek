import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { session } from "#lib/session.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import SearchList from "./SearchList.svelte";

const search = (id: string, minute: number, fields: Partial<Schemas["Search"]> = {}) =>
  ({
    id,
    searchText: `search ${id}`,
    token: 1,
    state: "InProgress",
    isComplete: false,
    startedAt: `2026-10-26T10:${String(minute).padStart(2, "0")}:00Z`,
    endedAt: null,
    responseCount: 0,
    fileCount: 0,
    lockedFileCount: 0,
    responseBytes: 0,
    responses: null,
    scope: { type: "network", subjects: [] },
    ...fields
  }) as Schemas["Search"];

const flush = async () => {
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
};

describe("the searches list", () => {
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    session.principal = { name: "me", role: "Administrator" } as never;
  });
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    live.stop();
    searches.clear();
    session.principal = null;
    vi.useRealTimers();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("keeps its rows and a text selection in them through frames and a reload", async () => {
    let stored = [search("a", 1), search("b", 2)];
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/searches") ? stored : undefined
    );
    live.start();
    component = mount(SearchList, { target: document.body });
    await flush();
    const row = (id: string) => document.querySelector<HTMLTableRowElement>(`tr[data-id="${id}"]`);
    const [a, b] = [row("a")!, row("b")!];
    const link = a.querySelector("a")!;
    document.getSelection()!.selectAllChildren(link);

    net.frame("search.update", search("a", 1, { fileCount: 5, responseCount: 1 }));
    net.frame("search.update", search("a", 1, { fileCount: 9, responseCount: 2 }));
    await flush();
    stored = [search("a", 1, { fileCount: 9 }), search("b", 2), search("c", 3)];
    await searches.load();
    await flush();

    expect(row("a")).toBe(a);
    expect(row("b")).toBe(b);
    expect(row("c")).not.toBeNull();
    expect(a.textContent).toContain("9");
    const selection = document.getSelection()!;
    expect(selection.anchorNode?.isConnected).toBe(true);
    expect(selection.toString()).toBe("search a");
  });
});
