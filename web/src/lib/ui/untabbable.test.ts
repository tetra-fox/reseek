import { afterEach, describe, expect, it, vi } from "vitest";

import { untabbable } from "./untabbable";

describe("a copy of controls kept out of the tab order", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("takes its controls out of the tab order, those added later too", async () => {
    const copy = document.createElement("div");
    copy.innerHTML = `<button type="button">a</button>`;
    document.body.append(copy);
    const stop = untabbable(() => {})(copy);
    expect(copy.querySelector("button")!.tabIndex).toBe(-1);
    copy.insertAdjacentHTML("beforeend", `<a href="#x">b</a>`);
    // the observer's records arrive as a microtask
    await Promise.resolve();
    expect(copy.querySelector("a")!.tabIndex).toBe(-1);
    stop();
  });

  it("hands focus that comes to a control on, and leaves a press its default", () => {
    const copy = document.createElement("div");
    copy.innerHTML = `<button type="button">a</button>`;
    document.body.append(copy);
    const refocus = vi.fn();
    const stop = untabbable(refocus)(copy);
    const button = copy.querySelector("button")!;
    const press = new MouseEvent("mousedown", { bubbles: true, cancelable: true });
    button.dispatchEvent(press);
    // a press may still start a text selection across the copy
    expect(press.defaultPrevented).toBe(false);
    // focus that arrives anyway, as a closing menu gives it back to its trigger
    button.focus();
    expect(refocus).toHaveBeenCalledTimes(1);
    stop();
  });
});
