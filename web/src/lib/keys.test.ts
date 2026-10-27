import { afterEach, describe, expect, it } from "vitest";

import { keepsKey, layerOpen } from "./keys";

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
  });
});
