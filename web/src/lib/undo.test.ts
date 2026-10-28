import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { toasts } from "./toast.svelte";
import { UNDO_MS, undoable } from "./undo";

describe("undo toasts", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    toasts.splice(0);
  });

  it("carry the action out once the toast has gone", () => {
    const commit = vi.fn();
    const undo = vi.fn();
    undoable("Removed 2 transfers", commit, undo);
    expect(toasts[0]).toMatchObject({ text: "Removed 2 transfers", action: { label: "Undo" } });
    vi.advanceTimersByTime(UNDO_MS - 1);
    expect(commit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(commit).toHaveBeenCalledOnce();
    expect(undo).not.toHaveBeenCalled();
  });

  it("put back what they undo and never carry it out", () => {
    const shown = new Set(["a", "b"]);
    shown.delete("a");
    const commit = vi.fn();
    undoable("Hid a", commit, () => shown.add("a"));
    toasts[0].action!.run();
    vi.advanceTimersByTime(UNDO_MS * 2);
    expect(shown.has("a")).toBe(true);
    expect(commit).not.toHaveBeenCalled();
  });

  it("carry out what waits when the page is left", () => {
    const commit = vi.fn();
    undoable("Deleted", commit, () => {});
    window.dispatchEvent(new Event("pagehide"));
    expect(commit).toHaveBeenCalledOnce();
    vi.advanceTimersByTime(UNDO_MS);
    expect(commit).toHaveBeenCalledOnce();
  });
});
