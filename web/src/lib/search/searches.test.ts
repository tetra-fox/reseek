import { afterEach, describe, expect, it, vi } from "vitest";

import { toasts } from "#lib/feedback/toast.svelte.js";
import { search } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { isDone, searchStage, searches, type SearchState } from "./searches.svelte";

describe("search stages", () => {
  it("read a state this page does not know as unknown, neither running nor done", () => {
    const newer = "Paused" as SearchState;
    expect(searchStage(newer)).toBe("unknown");
    expect(isDone(newer)).toBe(false);
    expect(searchStage("toString" as SearchState)).toBe("unknown");
    expect(isDone("Completed, Cancelled")).toBe(true);
    expect(isDone("Queued")).toBe(false);
  });
});

describe("searches deleted with an undo", () => {
  afterEach(() => {
    searches.clear();
    toasts.splice(0);
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("are all deleted at once with keepalive as the page is left", async () => {
    vi.useFakeTimers();
    const net = fakeNetwork(() => ({ status: 204 }));
    const ids = ["a", "b", "c"];
    for (const id of ids) searches.list.upsert(search({ id, state: "Completed, TimedOut" }));
    searches.removeLater(ids);
    window.dispatchEvent(new Event("pagehide"));
    await vi.advanceTimersByTimeAsync(0);
    expect(net.requests.map((r) => [r.method, r.url.pathname, r.keepalive])).toEqual(
      ids.map((id) => ["DELETE", `/api/v0/searches/${id}`, true])
    );
  });
});
