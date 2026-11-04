import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { flush } from "#lib/testing/flush.js";
import { press } from "#lib/testing/keys.js";
import Fixture from "#lib/testing/Suggest.svelte";

describe("a suggesting box's keys", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  /** A box holding text, its dropdown open and the highlight on the first entry */
  const start = async (value = "o") => {
    const onchoose = vi.fn();
    component = mount(Fixture, { target: document.body, props: { value, onchoose } });
    flushSync();
    const box = document.querySelector<HTMLInputElement>("[role=combobox]")!;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await flush();
    return { box, onchoose };
  };
  const entry = (text: string) =>
    [...document.querySelectorAll<HTMLElement>("[role=option]")].find(
      (e) => e.textContent.trim() === text
    )!;

  it("takes the first entry on Enter, before the dropdown has opened", () => {
    const onchoose = vi.fn();
    component = mount(Fixture, { target: document.body, props: { value: "o", onchoose } });
    flushSync();
    press(document.querySelector("[role=combobox]")!, "Enter");
    expect(onchoose).toHaveBeenCalledExactlyOnceWith("one");
  });

  it("takes the entry the keys moved to", async () => {
    const { box, onchoose } = await start();
    expect(entry("One").hasAttribute("data-highlighted")).toBe(true);
    press(box, "ArrowDown");
    await flush();
    expect(entry("Two").hasAttribute("data-highlighted")).toBe(true);
    press(box, "Enter");
    expect(onchoose).toHaveBeenCalledExactlyOnceWith("two");
  });

  it("goes past a disabled entry and comes back to the first", async () => {
    const { box, onchoose } = await start();
    press(box, "ArrowDown");
    press(box, "ArrowDown");
    press(box, "ArrowUp");
    await flush();
    press(box, "Enter");
    expect(onchoose).toHaveBeenCalledExactlyOnceWith("one");
  });

  it("leaves the highlight and Enter where they are when the pointer moves", async () => {
    const { box, onchoose } = await start();
    entry("Two").dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, cancelable: true, pointerType: "mouse" })
    );
    await flush();
    expect(entry("One").hasAttribute("data-highlighted")).toBe(true);
    press(box, "Enter");
    expect(onchoose).toHaveBeenCalledExactlyOnceWith("one");
  });

  it("takes an entry the pointer presses, and only that", async () => {
    const { onchoose } = await start();
    const target = entry("Two");
    const pointer = { bubbles: true, cancelable: true, pointerType: "mouse" };
    target.dispatchEvent(new PointerEvent("pointerdown", pointer));
    target.dispatchEvent(new PointerEvent("pointerup", pointer));
    expect(onchoose).toHaveBeenCalledExactlyOnceWith("two");
  });

  it("gives up an Enter that waits when the text changes meanwhile", async () => {
    let answer!: () => void;
    const settle = () => new Promise<void>((done) => (answer = done));
    const onchoose = vi.fn();
    component = mount(Fixture, { target: document.body, props: { value: "o", onchoose, settle } });
    flushSync();
    const box = document.querySelector<HTMLInputElement>("[role=combobox]")!;
    press(box, "Enter");
    box.value = "ot";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    answer();
    await flush();
    expect(onchoose).not.toHaveBeenCalled();
  });

  it("leaves Enter in an empty box to the form", () => {
    const onchoose = vi.fn();
    component = mount(Fixture, { target: document.body, props: { value: "  ", onchoose } });
    flushSync();
    const event = press(document.querySelector("[role=combobox]")!, "Enter");
    expect(event.defaultPrevented).toBe(false);
    expect(onchoose).not.toHaveBeenCalled();
  });
});
