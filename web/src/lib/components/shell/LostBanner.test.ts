import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";

import LostBanner from "./LostBanner.svelte";

describe("the lost connection banner", () => {
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => {
    live.status = "off";
    live.retryAt = null;
    vi.useRealTimers();
  });

  it("counts down from the retry's own wait, however long the page was up", () => {
    const banner = mount(LostBanner, { target: document.body });
    const text = () => document.body.textContent?.replace(/\s+/g, " ") ?? "";
    vi.advanceTimersByTime(19_000);
    live.status = "polling";
    live.retryAt = Date.now() + 1_000;
    flushSync();
    expect(text()).toContain("Retrying in 1s.");
    // the next retry is set between two ticks of the clock
    vi.advanceTimersByTime(1_500);
    live.retryAt = Date.now() + 4_000;
    flushSync();
    expect(text()).toContain("Retrying in 4s.");
    unmount(banner);
  });
});
