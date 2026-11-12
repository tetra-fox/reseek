import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { dismiss, hold, loadFailed, toast, toasts } from "./toast.svelte";

describe("toast countdowns", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    toasts.splice(0);
  });

  it("stop while held and run on with the time they had left, hold after hold", () => {
    const gone = vi.fn();
    const id = toast("Saved", "info", { ms: 6_000, onGone: gone });
    vi.advanceTimersByTime(2_000);
    hold(id, "pointer", true);
    expect(toasts[0].held).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(toasts).toHaveLength(1);
    hold(id, "pointer", false);
    expect(toasts[0].held).toBe(false);
    vi.advanceTimersByTime(1_000);
    hold(id, "pointer", true);
    vi.advanceTimersByTime(60_000);
    hold(id, "pointer", false);
    vi.advanceTimersByTime(2_999);
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

describe("toasts with a key", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    toasts.splice(0);
  });

  it("take the place of the one with their key, on top and with the whole time again", () => {
    const gone = vi.fn();
    const first = toast("New message from alice", "info", { key: "user:alice", onGone: gone });
    toast("Saved");
    vi.advanceTimersByTime(5_000);
    const again = toast("New message from alice", "info", { key: "user:alice" });
    expect(again).toBe(first);
    expect(gone).toHaveBeenCalledOnce();
    expect(toasts.map((t) => [t.text, t.round])).toEqual([
      ["New message from alice", 1],
      ["Saved", 0]
    ]);
    vi.advanceTimersByTime(5_999);
    expect(toasts.map((t) => t.text)).toEqual(["New message from alice"]);
    vi.advanceTimersByTime(1);
    expect(toasts).toHaveLength(0);
  });

  it("held when replaced, run their new countdown once let go", () => {
    const id = toast("New message from alice", "info", { key: "user:alice" });
    hold(id, "pointer", true);
    toast("New message from alice", "info", { key: "user:alice" });
    vi.advanceTimersByTime(60_000);
    expect(toasts).toHaveLength(1);
    hold(id, "pointer", false);
    vi.advanceTimersByTime(6_000);
    expect(toasts).toHaveLength(0);
  });
});

describe("a crowd of toasts", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    // the ones out of sight go too
    vi.runAllTimers();
    while (toasts.length) dismiss(toasts[0].id);
    vi.useRealTimers();
  });

  it("puts the oldest out of sight past five, errors last, each going when its time is up", () => {
    const gone = vi.fn();
    toast("Failed 1", "error");
    toast("Saved 1", "info", { onGone: () => gone("Saved 1") });
    toast("Failed 2", "error");
    toast("Saved 2", "info", { onGone: () => gone("Saved 2") });
    toast("Saved 3");
    vi.advanceTimersByTime(1_000);
    toast("Saved 4");
    expect(toasts.map((t) => t.text)).not.toContain("Saved 1");
    toast("Failed 3", "error");
    toast("Saved 5");
    expect(toasts.map((t) => t.text)).toEqual([
      "Saved 5",
      "Failed 3",
      "Saved 4",
      "Failed 2",
      "Failed 1"
    ]);
    // what the two were shown with, such as an undo's commit, waits for their own time
    vi.advanceTimersByTime(4_999);
    expect(gone).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(gone.mock.calls).toEqual([["Saved 1"], ["Saved 2"]]);
  });

  it("lets the oldest error go when every other one is an error, the newest kept", () => {
    for (const n of [1, 2, 3, 4, 5]) toast(`Failed ${n}`, "error");
    toast("Saved");
    expect(toasts.map((t) => t.text)).toEqual([
      "Saved",
      "Failed 5",
      "Failed 4",
      "Failed 3",
      "Failed 2"
    ]);
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
