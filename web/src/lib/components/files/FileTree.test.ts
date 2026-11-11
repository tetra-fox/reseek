import { flushSync, mount, unmount } from "svelte";
import { SvelteSet } from "svelte/reactivity";
import { afterEach, describe, expect, it, vi } from "vitest";

import { press } from "#lib/testing/keys.js";
import Tree from "#lib/testing/Tree.svelte";

describe("the file tree", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    vi.useRealTimers();
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const start = (folderPicks = false) => {
    const selected = new SvelteSet<string>();
    component = mount(Tree, { target: document.body, props: { selected, folderPicks } });
    flushSync();
    return selected;
  };
  const tree = () => document.querySelector<HTMLElement>("[role=treegrid]")!;
  /** the keys of the lines drawn, in order */
  const shown = () =>
    [...tree().querySelectorAll("[role=rowgroup] [data-line]")].map((l) =>
      l.getAttribute("data-line")
    );
  const line = (key: string) =>
    tree().querySelector<HTMLElement>(`[role=rowgroup] [data-line="${key}"]`)!;
  const menu = () => document.querySelector("[role=menu]")?.textContent?.trim() ?? null;

  it("opens the focused line's menu from the menu key and Shift+F10", async () => {
    start();
    press(tree(), "ArrowDown");
    press(tree(), "ArrowDown");
    press(tree(), "ContextMenu");
    await vi.waitFor(() => expect(menu()).toBe("Menu of folder:C"));
    press(document.querySelector("[role=menu]")!, "Escape");
    await vi.waitFor(() => expect(menu()).toBeNull());

    press(tree(), "ArrowUp");
    press(tree(), "F10", { shiftKey: true });
    await vi.waitFor(() => expect(menu()).toBe("Menu of folder:A"));
  });

  it("opens and closes a folder with Right and Left, and everything inside with Ctrl+Right", () => {
    start();
    press(tree(), "ArrowDown");
    expect(shown()).toEqual(["folder:A", "folder:C"]);
    press(tree(), "ArrowRight");
    expect(shown()).toEqual(["folder:A", "a1", "a2", "folder:A/B", "folder:C"]);
    press(tree(), "ArrowLeft");
    expect(shown()).toEqual(["folder:A", "folder:C"]);
    press(tree(), "ArrowRight", { ctrlKey: true });
    expect(shown()).toEqual(["folder:A", "a1", "a2", "folder:A/B", "b1", "folder:C"]);
  });

  it("selects all the files in closed folders, those inside them included", () => {
    const selected = start();
    tree().querySelector<HTMLElement>('[aria-label="Select all"]')!.click();
    flushSync();
    expect([...selected].toSorted()).toEqual(["a1", "a2", "b1", "c1"]);

    // with A open, its files come from its lines and B's from B, still closed
    selected.clear();
    press(tree(), "ArrowDown");
    press(tree(), "ArrowRight");
    tree().querySelector<HTMLElement>('[aria-label="Select all"]')!.click();
    flushSync();
    expect([...selected].toSorted()).toEqual(["a1", "a2", "b1", "c1"]);
  });

  it("selects the folders on the lines when folders are picked like files", () => {
    const selected = start(true);
    tree().querySelector<HTMLElement>('[aria-label="Select all"]')!.click();
    flushSync();
    expect([...selected].toSorted()).toEqual(["folder:A", "folder:C"]);
  });

  it("opens a line's menu on a long press, and the click after it selects nothing", async () => {
    vi.useFakeTimers();
    start(true);
    const touch = { bubbles: true, pointerType: "touch", clientX: 5, clientY: 5 };
    // a short press opens nothing
    line("folder:C").dispatchEvent(new PointerEvent("pointerdown", touch));
    line("folder:C").dispatchEvent(new PointerEvent("pointerup", touch));
    await vi.advanceTimersByTimeAsync(1000);
    flushSync();
    expect(menu()).toBeNull();

    line("folder:C").dispatchEvent(new PointerEvent("pointerdown", touch));
    await vi.advanceTimersByTimeAsync(500);
    flushSync();
    expect(menu()).toBe("Menu of folder:C");
    line("folder:C").click();
    flushSync();
    expect(line("folder:C").getAttribute("aria-selected")).toBe("false");
  });
});
