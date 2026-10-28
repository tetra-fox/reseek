import { SvelteSet } from "svelte/reactivity";

/**
 * Which folders of a tree, or sections of an accordion, are open. Each starts as `openByDefault`
 * says and is toggled on its own; Alt-click toggles it and every sibling to the same state, as
 * file managers do
 */
export class Expansion {
  openByDefault = $state(true);
  /** the keys whose state differs from the default */
  #flipped = new SvelteSet<string>();

  constructor(openByDefault: boolean) {
    this.openByDefault = openByDefault;
  }

  isOpen = (key: string) => this.#flipped.has(key) !== this.openByDefault;

  set(key: string, open: boolean) {
    if (open === this.openByDefault) this.#flipped.delete(key);
    else this.#flipped.add(key);
  }

  /**
   * Opens a closed one or closes an open one. With `all`, its `siblings` (itself among them or
   * not) all take the state it takes
   */
  toggle(key: string, siblings: string[] = [], all = false) {
    const open = !this.isOpen(key);
    this.set(key, open);
    if (all) for (const sibling of siblings) this.set(sibling, open);
  }

  /** Every key back to one state, as Expand all and Collapse all do */
  reset(openByDefault: boolean) {
    this.openByDefault = openByDefault;
    this.#flipped.clear();
  }
}

/** Whether a click asks for the siblings too: Alt, or Option on a Mac */
export const withSiblings = (e: MouseEvent | KeyboardEvent) => e.altKey;
