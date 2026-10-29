import { afterEach, describe, expect, it, vi } from "vitest";

import { Arrivals, FRESH_MS } from "./arrivals.svelte";

describe("arrivals", () => {
  afterEach(() => vi.useRealTimers());

  it("mark the lines a change brings, again when they come back, until their entrance is over", () => {
    vi.useFakeTimers();
    const arrivals = new Arrivals();
    arrivals.see(["a", "b"]);
    expect(arrivals.isFresh("a")).toBe(false);
    // a folder opens under b
    arrivals.see(["a", "b", "b1", "b2"]);
    expect(["a", "b1", "b2"].map(arrivals.isFresh)).toEqual([false, true, true]);
    vi.advanceTimersByTime(FRESH_MS);
    expect(arrivals.isFresh("b1")).toBe(false);
    // closed and opened again, its lines make their entrance again
    arrivals.see(["a", "b"]);
    arrivals.see(["a", "b", "b1", "b2"]);
    expect(arrivals.isFresh("b2")).toBe(true);
  });
});
