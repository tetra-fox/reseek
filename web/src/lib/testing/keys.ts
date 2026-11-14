import { flushSync } from "svelte";

/** Presses `key` on `target`, which bubbles to the list it is in */
export const press = (target: Element, key: string, mods: KeyboardEventInit = {}) => {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...mods });
  target.dispatchEvent(event);
  flushSync();
  return event;
};

/**
 * Gives `target` focus as the keys would, whatever the tab order: a Tab goes down where focus is,
 * then `target` takes it, which jsdom then counts as focus from the keys (`:focus-visible`)
 */
export const focusByKeys = (target: HTMLElement) => {
  const from = document.activeElement ?? document.body;
  from.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }));
  target.focus();
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

/**
 * Presses Enter in a form's field the way a browser does, which jsdom doesn't: the form's
 * default button is clicked, unless it is disabled, and a form without one is submitted. The
 * page isn't brought up to date first, as when Enter comes right after the last keystroke
 */
export const pressEnter = (field: HTMLInputElement) => {
  const event = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  field.dispatchEvent(event);
  const form = field.form;
  if (!event.defaultPrevented && form) {
    const button = form.querySelector<HTMLButtonElement>(
      'button[type="submit"], button:not([type])'
    );
    if (!button) form.requestSubmit();
    else if (!button.disabled) button.click();
  }
  flushSync();
};
