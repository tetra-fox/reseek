import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";

import ResizeHandle from "./ResizeHandle.svelte";

afterEach(() => {
  document.body.innerHTML = "";
});

it("reports and moves from the size in effect when a kept size is past the greatest", () => {
  const onchange = vi.fn();
  // a width kept from a wider window than the one now
  const component = mount(ResizeHandle, {
    target: document.body,
    props: { value: 900, min: 200, max: 600, initial: 300, edge: "left", label: "Width", onchange }
  });
  flushSync();
  const handle = document.querySelector<HTMLElement>('[role="separator"]')!;
  expect(handle.getAttribute("aria-valuenow")).toBe("600");
  // the edge on the left: the right arrow narrows it
  handle.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
  flushSync();
  expect(onchange).toHaveBeenCalledWith(580);
  unmount(component);
});
