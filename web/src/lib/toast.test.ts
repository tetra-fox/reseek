import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dismiss, hold, loadFailed, toast, toasts } from "./toast.svelte";

describe("toast countdowns", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    toasts.splice(0);
  });

  it("stop while held and run on with the time they had left", () => {
    const gone = vi.fn();
    const id = toast("Saved", "info", { ms: 6_000, onGone: gone });
    vi.advanceTimersByTime(2_000);
    hold(id, "pointer", true);
    expect(toasts[0].held).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(toasts).toHaveLength(1);
    hold(id, "pointer", false);
    expect(toasts[0].held).toBe(false);
    vi.advanceTimersByTime(3_999);
    expect(toasts).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(toasts).toHaveLength(0);
    expect(gone).toHaveBeenCalledOnce();
  });

  it("stay held until neither the pointer nor focus holds them", () => {
    const id = toast("Saved", "info", { ms: 1_000 });
    hold(id, "pointer", true);
    hold(id, "focus", true);
    hold(id, "pointer", false);
    vi.advanceTimersByTime(5_000);
    expect(toasts).toHaveLength(1);
    hold(id, "focus", false);
    vi.advanceTimersByTime(1_000);
    expect(toasts).toHaveLength(0);
  });

  it("keep the time left across several holds", () => {
    const id = toast("Saved", "info", { ms: 3_000 });
    for (let i = 0; i < 2; i++) {
      vi.advanceTimersByTime(1_000);
      hold(id, "pointer", true);
      vi.advanceTimersByTime(10_000);
      hold(id, "pointer", false);
    }
    vi.advanceTimersByTime(999);
    expect(toasts).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(toasts).toHaveLength(0);
  });

  it("that stay until dismissed never go by themselves", () => {
    const gone = vi.fn();
    const id = toast("Failed", "error", { ms: Infinity, onGone: gone });
    vi.advanceTimersByTime(24 * 3600_000);
    expect(toasts).toHaveLength(1);
    dismiss(id);
    expect(toasts).toHaveLength(0);
    expect(gone).toHaveBeenCalledOnce();
  });
});

describe("a failed load", () => {
  afterEach(() => toasts.splice(0));

  it("is the page's to show the first time, and says so once per outage after that", () => {
    const list = { loaded: false, error: "" };
    loadFailed(list, "database is locked");
    expect(list.error).toBe("database is locked");
    expect(toasts).toHaveLength(0);
    Object.assign(list, { loaded: true, error: "" });
    loadFailed(list, "database is locked");
    loadFailed(list, "database is locked");
    expect(toasts.map((t) => t.text)).toEqual(["database is locked"]);
    list.error = "";
    loadFailed(list, "database is locked");
    expect(toasts).toHaveLength(2);
  });
});
