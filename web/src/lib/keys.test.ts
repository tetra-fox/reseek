import { afterEach, describe, expect, it } from "vitest";

import { keepsKey } from "./keys";

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
    const at = (id: string) => document.getElementById(id);
    for (const key of ["Delete", "a", "ArrowDown", "Enter"]) {
      expect(keepsKey(at("field"), key)).toBe(true);
      expect(keepsKey(at("item"), key)).toBe(true);
      expect(keepsKey(at("cell"), key)).toBe(false);
    }
    for (const id of ["button", "inside", "box", "link"]) {
      expect(keepsKey(at(id), "Enter")).toBe(true);
      expect(keepsKey(at(id), " ")).toBe(true);
      expect(keepsKey(at(id), "Delete")).toBe(false);
      expect(keepsKey(at(id), "ArrowDown")).toBe(false);
    }
  });
});
