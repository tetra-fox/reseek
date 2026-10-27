import { flushSync } from "svelte";

/** Presses `key` on `target`, which bubbles to the list it is in */
export const press = (target: Element, key: string, mods: KeyboardEventInit = {}) => {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...mods });
  target.dispatchEvent(event);
  flushSync();
  return event;
};

/** The keys of the selected lines in `root` */
export const ticked = (root: ParentNode = document) =>
  [...root.querySelectorAll('[data-line][aria-selected="true"]')].map((l) =>
    l.getAttribute("data-line")
  );

/** The key of the line `aria-activedescendant` names in the list `grid` */
export const active = (grid: Element) =>
  document
    .getElementById(grid.getAttribute("aria-activedescendant") ?? "")
    ?.getAttribute("data-line") ?? null;
