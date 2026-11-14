import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Freeze, freezesWhileUsed, interaction, SELECTION_IDLE_MS } from "./interaction.svelte";
import { Keyed } from "./keyed.svelte";

type Row = { id: string };

/** A list of rows `a` and `b` drawn in an element of its own, which a selection holds */
const list = () => {
  const rows = new Keyed<Row>((row) => row.id);
  rows.reconcile([{ id: "a" }, { id: "b" }]);
  const element = document.body.appendChild(document.createElement("div"));
  element.textContent = "a b";
  const detach = freezesWhileUsed(rows.freeze)(element);
  return { rows, element, detach };
};

/** Selects the text of `element`, as a drag or a double click does */
const select = (element: Element) => {
  document.getSelection()!.selectAllChildren(element);
  document.dispatchEvent(new Event("selectionchange"));
};

const keys = (rows: Keyed<Row>) => rows.rows.map((row) => row.id);

describe("a text selection", () => {
  const detached: (() => void)[] = [];
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    for (const detach of detached.splice(0)) detach();
    document.getSelection()!.removeAllRanges();
    document.body.innerHTML = "";
    vi.useRealTimers();
  });

  it("holds back removals in its own list only", () => {
    const chat = list();
    const other = list();
    detached.push(chat.detach, other.detach);
    select(chat.element);
    chat.rows.remove("a");
    other.rows.remove("a");
    expect(keys(chat.rows)).toEqual(["a", "b"]);
    expect(chat.rows.removed.has("a")).toBe(true);
    expect(keys(other.rows)).toEqual(["b"]);
    expect(interaction.frozen).toBe(false);

    document.getSelection()!.removeAllRanges();
    document.dispatchEvent(new Event("selectionchange"));
    expect(keys(chat.rows)).toEqual(["b"]);
  });

  it("lets its list catch up after a while with no pointer or key activity", () => {
    const chat = list();
    detached.push(chat.detach);
    select(chat.element);
    chat.rows.remove("a");

    // the user moving the pointer or typing keeps it held
    vi.advanceTimersByTime(SELECTION_IDLE_MS - 1_000);
    window.dispatchEvent(new PointerEvent("pointermove"));
    vi.advanceTimersByTime(SELECTION_IDLE_MS - 1_000);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Shift" }));
    vi.advanceTimersByTime(SELECTION_IDLE_MS - 1_000);
    expect(keys(chat.rows)).toEqual(["a", "b"]);

    // then nothing for the whole while: the removal waits no longer, though the text is selected
    vi.advanceTimersByTime(1_000);
    expect(keys(chat.rows)).toEqual(["b"]);
    expect(chat.rows.freeze.frozen).toBe(false);

    // the user back at the page, with the text still selected, holds the list again
    window.dispatchEvent(new PointerEvent("pointermove"));
    expect(chat.rows.freeze.frozen).toBe(true);
    chat.rows.remove("b");
    expect(keys(chat.rows)).toEqual(["b"]);
  });

  it("stops watching for activity once the selection leaves its list", () => {
    const chat = list();
    detached.push(chat.detach);
    select(chat.element);
    document.getSelection()!.removeAllRanges();
    document.dispatchEvent(new Event("selectionchange"));
    window.dispatchEvent(new PointerEvent("pointermove"));
    expect(chat.rows.freeze.frozen).toBe(false);
  });
});

describe("a list's freeze", () => {
  it("follows what holds every list, and runs what waited once both let go", () => {
    const freeze = new Freeze();
    const run = vi.fn();
    const everyList = interaction.hold();
    expect(freeze.frozen).toBe(true);
    const own = freeze.hold();
    freeze.whenThawed(run);
    everyList();
    expect(run).not.toHaveBeenCalled();
    const again = interaction.hold();
    own();
    expect(run).not.toHaveBeenCalled();
    again();
    expect(run).toHaveBeenCalledOnce();
  });
});
