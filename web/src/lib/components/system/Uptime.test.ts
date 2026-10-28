import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import Uptime from "./Uptime.svelte";

describe("the uptime", () => {
  afterEach(() => vi.useRealTimers());

  it("counts each second on the shared clock", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-28T13:12:05Z"));
    const component = mount(Uptime, {
      target: document.body,
      props: { since: "2026-10-28T10:00:00Z" }
    });
    flushSync();
    expect(document.body.textContent).toBe("3h 12m 5s");
    vi.advanceTimersByTime(1000);
    flushSync();
    expect(document.body.textContent).toBe("3h 12m 6s");
    // past a day the seconds give way to the days
    vi.advanceTimersByTime(86_400_000);
    flushSync();
    expect(document.body.textContent).toBe("1d 3h 12m");
    unmount(component);
    document.body.innerHTML = "";
  });
});
