import { SvelteSet } from "svelte/reactivity";

import { mac } from "#lib/ui/keys.js";

/**
 * Which folders of a tree, or sections of an accordion, are open. Each starts as `openByDefault`
 * says and is toggled on its own, or together with others: its siblings on an Alt-click, every
 * folder inside it on a Ctrl-click
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

  /** Opens a closed one or closes an open one; the keys in `also` all take the state it takes */
  toggle(key: string, also: string[] = []) {
    const open = !this.isOpen(key);
    this.set(key, open);
    for (const other of also) this.set(other, open);
  }

  /** Every key back to one state, as Expand all and Collapse all do */
  reset(openByDefault: boolean) {
    this.openByDefault = openByDefault;
    this.#flipped.clear();
  }

  /** What is open, to keep: the default and the keys that differ from it */
  snapshot(): ExpansionSnapshot {
    return { openByDefault: this.openByDefault, flipped: [...this.#flipped] };
  }

  /** Back to what a snapshot kept */
  restore({ openByDefault, flipped }: ExpansionSnapshot) {
    this.reset(openByDefault);
    for (const key of flipped) this.#flipped.add(key);
  }
}

export type ExpansionSnapshot = { openByDefault: boolean; flipped: string[] };

/** Whether a click asks for the siblings too: Alt, or Option on a Mac */
export const withSiblings = (e: MouseEvent | KeyboardEvent) => e.altKey;

/**
 * Whether a click or key asks for every folder inside too: Ctrl, or Cmd on a Mac, where a
 * Ctrl-click is a right-click
 */
export const withInside = (e: MouseEvent | KeyboardEvent) => (mac ? e.metaKey : e.ctrlKey);

/** The keys a toggle with `e` takes along: the siblings, the folders inside, or none */
export const alsoToggled = (
  e: MouseEvent | KeyboardEvent,
  key: string,
  siblings: string[],
  inside: () => string[]
) => (withSiblings(e) ? siblings.filter((s) => s !== key) : withInside(e) ? inside() : []);
