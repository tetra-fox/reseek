import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { press, ticked } from "#lib/testing/keys.js";
import List from "#lib/testing/List.svelte";
import { box } from "#lib/testing/state.svelte.js";

type Line = { key: string; group: string; header?: boolean };

const grouped = (): Line[] => [
  { key: "h:a", group: "a", header: true },
  { key: "a1", group: "a" },
  { key: "a2", group: "a" },
  { key: "h:b", group: "b", header: true },
  { key: "b1", group: "b" }
];

describe("a list's selection and focus", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const start = (lines: Line[], props: Record<string, unknown> = {}) => {
    const source = box(lines);
    component = mount(List, { target: document.body, props: { source, ...props } });
    flushSync();
    return source;
  };
  const grid = () => document.querySelector<HTMLElement>("[role=grid]")!;
  const line = (key: string) => document.querySelector<HTMLElement>(`[data-line="${key}"]`)!;
  const focused = () => document.querySelector(".focused")?.getAttribute("data-line") ?? null;

  it("moves by position over the rows, past the group headers", () => {
    start(grouped());
    press(grid(), "ArrowDown");
    expect(focused()).toBe("a1");
    press(grid(), "ArrowDown");
    press(grid(), "ArrowDown");
    expect(focused()).toBe("b1");
    // nothing below the last row
    press(grid(), "ArrowDown");
    expect(focused()).toBe("b1");
    press(grid(), "ArrowUp");
    expect(focused()).toBe("a2");
    expect(document.getElementById(grid().getAttribute("aria-activedescendant")!)).toBe(line("a2"));
  });

  it("goes to the ends and a page at a time, and stops on headers that take focus", () => {
    start(grouped());
    press(grid(), "End");
    expect(focused()).toBe("b1");
    press(grid(), "PageUp");
    expect(focused()).toBe("a2");
    press(grid(), "Home");
    expect(focused()).toBe("a1");
    if (component) unmount(component);
    start(grouped(), { headersFocus: true });
    press(grid(), "ArrowDown");
    expect(focused()).toBe("h:a");
    // Space on a header ticks its group, and it stays out of the selection itself
    press(grid(), " ");
    expect(ticked()).toEqual(["a1", "a2"]);
    press(grid(), "PageDown");
    expect(focused()).toBe("a2");
  });

  it("finds a line by the letters typed", () => {
    start(
      [
        { key: "apple", group: "x" },
        { key: "banana", group: "x" },
        { key: "blueberry", group: "x" }
      ],
      { named: true }
    );
    press(grid(), "b");
    expect(focused()).toBe("banana");
    press(grid(), "l");
    expect(focused()).toBe("blueberry");
  });

  it("shows the focused line's ring after a key, not after a press, and not once focus leaves", () => {
    start(grouped());
    press(grid(), "ArrowDown");
    expect(focused()).toBe("a1");
    line("a2").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    flushSync();
    expect(focused()).toBe(null);
    press(grid(), "ArrowDown");
    expect(focused()).toBe("b1");
    grid().dispatchEvent(
      new FocusEvent("focusout", { bubbles: true, relatedTarget: document.body })
    );
    flushSync();
    expect(focused()).toBe(null);
  });

  it("toggles with Space, opens with Enter and selects the group with Ctrl or Cmd+A", () => {
    const onopen = vi.fn();
    start(grouped(), { onopen });
    press(grid(), "ArrowDown");
    press(grid(), " ");
    expect(ticked()).toEqual(["a1"]);
    press(grid(), "Enter");
    expect(onopen).toHaveBeenCalledWith(expect.objectContaining({ key: "a1" }));
    press(grid(), " ");
    expect(ticked()).toEqual([]);
    expect(press(grid(), "a", { metaKey: true }).defaultPrevented).toBe(true);
    expect(ticked()).toEqual(["a1", "a2"]);
  });

  it("selects every row with Ctrl+A when nothing has focus", () => {
    start(grouped());
    press(grid(), "a", { ctrlKey: true });
    expect(ticked()).toEqual(["a1", "a2", "b1"]);
  });

  it("acts on a group header with Ctrl+A and Space only", () => {
    const onopen = vi.fn();
    start(grouped(), { onopen });
    line("h:b").dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    press(grid(), " ");
    expect(ticked()).toEqual(["b1"]);
    press(grid(), " ");
    expect(ticked()).toEqual([]);
    expect(press(grid(), "Enter").defaultPrevented).toBe(false);
    expect(onopen).not.toHaveBeenCalled();
  });

  it("leaves keys with Ctrl, Cmd or Alt to the browser, Ctrl+A aside", () => {
    start(grouped());
    press(grid(), "ArrowDown");
    for (const mods of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }]) {
      expect(press(grid(), "ArrowDown", mods).defaultPrevented).toBe(false);
      expect(press(grid(), " ", mods).defaultPrevented).toBe(false);
    }
    expect(focused()).toBe("a1");
    expect(ticked()).toEqual([]);
  });

  it("leaves Enter and Space to a focused control and acts on its row with the rest", () => {
    start(grouped());
    const button = line("a2").querySelector("button:not([role])")!;
    button.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(press(button, " ").defaultPrevented).toBe(false);
    expect(ticked()).toEqual([]);
    expect(press(button, "ArrowDown").defaultPrevented).toBe(true);
    expect(focused()).toBe("b1");
  });

  it("selects a range from the anchor with Shift on a checkbox", () => {
    start(grouped());
    const box = (key: string) => line(key).querySelector<HTMLElement>("[role=checkbox]")!;
    box("a1").click();
    flushSync();
    box("b1").dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
    flushSync();
    expect(ticked()).toEqual(["a1", "a2", "b1"]);
  });

  it("closes what is open with Escape before it clears the selection", () => {
    let drawer = true;
    start(grouped(), { layer: () => drawer });
    press(grid(), "ArrowDown");
    press(grid(), " ");
    expect(press(grid(), "Escape").defaultPrevented).toBe(false);
    expect(ticked()).toEqual(["a1"]);
    drawer = false;
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    document.body.append(menu);
    press(grid(), "Escape");
    expect(ticked()).toEqual(["a1"]);
    menu.remove();
    expect(press(grid(), "Escape").defaultPrevented).toBe(true);
    expect(ticked()).toEqual([]);
  });

  it("passes a removed row's place to the next row, else the one before", () => {
    const source = start(grouped());
    press(grid(), "ArrowDown");
    press(grid(), "ArrowDown");
    source.value = source.value.filter((l) => l.key !== "a2");
    flushSync();
    // the header of b is in a2's place now, so the row after it takes over
    expect(focused()).toBe("b1");
    source.value = source.value.filter((l) => l.key !== "b1");
    flushSync();
    expect(focused()).toBe("a1");
  });

  it("takes focus back when the line holding it leaves the list", () => {
    const source = start(grouped());
    const button = line("a1").querySelector<HTMLElement>("button:not([role])")!;
    button.focus();
    source.value = source.value.filter((l) => l.key !== "a1");
    flushSync();
    expect(document.activeElement).toBe(grid());
  });

  it("leaves focus the user put outside the list when the line it was in leaves", () => {
    const source = start(grouped());
    const outside = document.createElement("button");
    document.body.append(outside);
    const button = line("a1").querySelector<HTMLElement>("button:not([role])")!;
    button.focus();
    outside.focus();
    outside.blur();
    expect(document.activeElement).toBe(document.body);
    source.value = source.value.filter((l) => l.key !== "a1");
    flushSync();
    expect(document.activeElement).toBe(document.body);
  });

  it("selects a clicked row alone, toggles with Ctrl or Cmd and extends with Shift", () => {
    start(grouped());
    const click = (key: string, mods: MouseEventInit = {}) => {
      line(key).dispatchEvent(new MouseEvent("click", { bubbles: true, ...mods }));
      flushSync();
    };
    click("a1");
    click("a2");
    expect(ticked()).toEqual(["a2"]);
    click("b1", { ctrlKey: true });
    expect(ticked()).toEqual(["a2", "b1"]);
    click("b1", { metaKey: true });
    expect(ticked()).toEqual(["a2"]);
    // the anchor is the row last clicked on its own: b1, from the Cmd-click
    click("a1", { shiftKey: true });
    expect(ticked()).toEqual(["a1", "a2", "b1"]);
    click("a1");
    click("a2", { shiftKey: true, ctrlKey: true });
    expect(ticked()).toEqual(["a1", "a2"]);
    // a click on a control or a header leaves the selection as it is
    line("b1").querySelector<HTMLElement>("button:not([role])")!.click();
    click("h:b");
    expect(ticked()).toEqual(["a1", "a2"]);
  });
});
