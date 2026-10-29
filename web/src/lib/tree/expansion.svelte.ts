import { SvelteSet } from "svelte/reactivity";

/**
 * Which folders of a tree, or sections of an accordion, are open. Each starts as `openByDefault`
 * says, or as `defaultFor` says for the keys it answers, and is toggled on its own; Alt-click
 * toggles it and every sibling to the same state, as file managers do
 */
export class Expansion {
  openByDefault = $state(true);
  /** the keys whose state differs from their default */
  #flipped = new SvelteSet<string>();
  #defaultFor: (key: string) => boolean | undefined;

  constructor(
    openByDefault: boolean,
    defaultFor: (key: string) => boolean | undefined = () => undefined
  ) {
    this.openByDefault = openByDefault;
    this.#defaultFor = defaultFor;
  }

  #default = (key: string) => this.#defaultFor(key) ?? this.openByDefault;

  isOpen = (key: string) => this.#flipped.has(key) !== this.#default(key);

  set(key: string, open: boolean) {
    if (open === this.#default(key)) this.#flipped.delete(key);
    else this.#flipped.add(key);
  }

  /** The keys `test` picks back to their default */
  forget(test: (key: string) => boolean) {
    for (const key of this.#flipped) if (test(key)) this.#flipped.delete(key);
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
