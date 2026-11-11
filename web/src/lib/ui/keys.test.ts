import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import Select from "#lib/components/ui/Select.svelte";

import { APP, CHAT, DETAILS, LIST } from "./bindings";
import { isPressed, keepsKey, keysAria, keysText, keyStep, layerOpen } from "./keys";

describe("which keys a focused element keeps", () => {
  afterEach(() => (document.body.innerHTML = ""));

  it("keeps every key in a field or a menu, and Enter and Space on a control", () => {
    document.body.innerHTML = `
      <input id="field" />
      <div role="menu"><div role="menuitem" id="item"></div></div>
      <button id="button"><span id="inside"></span></button>
      <button role="checkbox" id="box"></button>
      <a href="#/users/a" id="link">a</a>
      <div id="cell"></div>`;
    const keeps = (id: string, key: string) => {
      const event = new KeyboardEvent("keydown", { key, bubbles: true });
      let kept = false;
      document.body.addEventListener("keydown", (e) => (kept = keepsKey(e)), { once: true });
      document.getElementById(id)!.dispatchEvent(event);
      return kept;
    };
    for (const key of ["Delete", "a", "ArrowDown", "Enter"]) {
      expect(keeps("field", key)).toBe(true);
      expect(keeps("item", key)).toBe(true);
      expect(keeps("cell", key)).toBe(false);
    }
    for (const id of ["button", "inside", "box", "link"]) {
      expect(keeps(id, "Enter")).toBe(true);
      expect(keeps(id, " ")).toBe(true);
      expect(keeps(id, "Delete")).toBe(false);
      expect(keeps(id, "ArrowDown")).toBe(false);
    }
  });

  it("leaves a key something already handled", () => {
    document.body.innerHTML = `<div id="cell"></div>`;
    let kept = false;
    document.body.addEventListener("keydown", (e) => (kept = keepsKey(e)), { once: true });
    const cell = document.getElementById("cell")!;
    cell.addEventListener("keydown", (e) => e.preventDefault(), { once: true });
    cell.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true })
    );
    expect(kept).toBe(true);
  });

  it("doesn't take a dialog or menu around the list itself for a control in it", () => {
    document.body.innerHTML = `
      <div role="dialog"><div role="grid" id="list"><div id="row"></div><button id="act"></button></div></div>`;
    const list = document.getElementById("list")!;
    let kept: boolean | null = null;
    list.addEventListener("keydown", (e) => (kept = keepsKey(e)));
    document
      .getElementById("row")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(kept).toBe(false);
    document
      .getElementById("act")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    expect(kept).toBe(true);
  });
});

describe("an open layer", () => {
  afterEach(() => (document.body.innerHTML = ""));

  it("counts a menu or dialog over the page until it starts closing", () => {
    document.body.innerHTML = `<div role="menu" data-state="open"></div>`;
    expect(layerOpen()).toBe(true);
    // still in the page for its exit animation
    document.querySelector("[role=menu]")!.setAttribute("data-state", "closed");
    expect(layerOpen()).toBe(false);
    document.body.innerHTML = `<div role="dialog"></div>`;
    expect(layerOpen()).toBe(true);
    // a list of options that is part of the page, as the composer's suggestions, is not one
    document.body.innerHTML = `<div role="listbox"></div>`;
    expect(layerOpen()).toBe(false);
  });

  it("counts a select's list while it is open", () => {
    document.body.innerHTML = "";
    const select = mount(Select, {
      target: document.body,
      props: { "value": "", "options": [{ value: "", label: "Any group" }], "aria-label": "Group" }
    });
    flushSync();
    expect(layerOpen()).toBe(false);
    const trigger = document.querySelector<HTMLElement>('[aria-label="Group"]')!;
    trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();
    expect(document.querySelector('[role="listbox"]')).not.toBeNull();
    expect(layerOpen()).toBe(true);
    unmount(select);
  });
});

describe("a key binding", () => {
  const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

  it("takes Ctrl or Cmd for the palette and nothing near it", () => {
    expect(isPressed(APP.palette, key({ key: "k", ctrlKey: true }))).toBe(true);
    expect(isPressed(APP.palette, key({ key: "K", metaKey: true }))).toBe(true);
    expect(isPressed(APP.palette, key({ key: "k" }))).toBe(false);
    expect(isPressed(APP.palette, key({ key: "k", ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isPressed(APP.palette, key({ key: "k", ctrlKey: true, altKey: true }))).toBe(false);
    expect(isPressed(APP.palette, key({ key: "j", ctrlKey: true }))).toBe(false);
  });

  it("takes a key typed with Shift, unless it says how Shift is held", () => {
    // `?` is Shift+/ on most keyboards and a key of its own on some
    expect(isPressed(APP.shortcuts, key({ key: "?", shiftKey: true }))).toBe(true);
    expect(isPressed(APP.shortcuts, key({ key: "?" }))).toBe(true);
    expect(isPressed(APP.shortcuts, key({ key: "?", ctrlKey: true }))).toBe(false);
    // Space shows details and Shift+Space ticks the row
    expect(isPressed(DETAILS.details, key({ key: " " }))).toBe(true);
    expect(isPressed(DETAILS.details, key({ key: " ", shiftKey: true }))).toBe(false);
    expect(isPressed(DETAILS.select, key({ key: " " }))).toBe(false);
    expect(isPressed(LIST.select, key({ key: " ", shiftKey: true }))).toBe(true);
  });

  it("tells the two ways of a pair apart", () => {
    expect(keyStep(LIST.move, key({ key: "ArrowUp" }))).toBe(-1);
    expect(keyStep(LIST.move, key({ key: "ArrowDown", shiftKey: true }))).toBe(1);
    expect(keyStep(LIST.move, key({ key: "ArrowDown", altKey: true }))).toBe(0);
    expect(keyStep(CHAT.movePin, key({ key: "ArrowDown", altKey: true, shiftKey: true }))).toBe(1);
    expect(keyStep(CHAT.next, key({ key: "ArrowDown", altKey: true, shiftKey: true }))).toBe(0);
  });

  it("names its keys as a Mac's keyboard does, and as others do", () => {
    expect(keysText(APP.palette, true)).toBe("⌘K");
    expect(keysText(APP.palette, false)).toBe("Ctrl+K");
    expect(keysText(CHAT.movePin, true)).toBe("⌥⇧↑ or ⌥⇧↓");
    expect(keysText(CHAT.movePin, false)).toBe("Alt+Shift+↑ or Alt+Shift+↓");
    expect(keysAria(APP.palette, true)).toBe("Meta+K");
    expect(keysAria(LIST.menu, false)).toBe("ContextMenu Shift+F10");
  });
});
