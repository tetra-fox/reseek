import { mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { searches } from "#lib/search/searches.svelte.js";
import { session } from "#lib/session.svelte.js";
import { search } from "#lib/testing/fixtures.js";
import { flushFaked } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import SearchList from "./SearchList.svelte";

/** A search named after `id`, started `minute` minutes past ten */
const listed = (id: string, minute: number, fields: Partial<Schemas["Search"]> = {}) =>
  search({
    id,
    searchText: `search ${id}`,
    startedAt: `2026-10-26T10:${String(minute).padStart(2, "0")}:00Z`,
    ...fields
  });

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
    let stored = [listed("a", 1), listed("b", 2)];
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/searches") ? stored : undefined
    );
    live.start();
    component = mount(SearchList, { target: document.body });
    await flushFaked();
    const row = (id: string) => document.querySelector<HTMLTableRowElement>(`tr[data-id="${id}"]`);
    const [a, b] = [row("a")!, row("b")!];
    const link = a.querySelector("a")!;
    document.getSelection()!.selectAllChildren(link);

    net.frame("search.update", listed("a", 1, { fileCount: 5, responseCount: 1 }));
    net.frame("search.update", listed("a", 1, { fileCount: 9, responseCount: 2 }));
    await flushFaked();
    stored = [listed("a", 1, { fileCount: 9 }), listed("b", 2), listed("c", 3)];
    await searches.load();
    await flushFaked();

    expect(row("a")).toBe(a);
    expect(row("b")).toBe(b);
    expect(row("c")).not.toBeNull();
    expect(a.textContent).toContain("9");
    const selection = document.getSelection()!;
    expect(selection.anchorNode?.isConnected).toBe(true);
    expect(selection.toString()).toBe("search a");
  });
});
