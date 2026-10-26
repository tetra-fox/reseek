import { untrack } from "svelte";

/**
 * Whether the user is in the middle of something a list must not move under: a text selection
 * in it, a pointer pressed on it, a hover card or menu open from it. While anything holds it,
 * lists take appends only and keep removed rows until it ends
 */
class Interaction {
  #holds = $state(0);
  #thawed: (() => void)[] = [];

  get frozen(): boolean {
    return this.#holds > 0;
  }

  /** Freezes until the returned function is called. Callable from an effect: the count it
   * changes is not one the effect follows */
  hold(): () => void {
    untrack(() => (this.#holds += 1));
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const left = untrack(() => (this.#holds -= 1));
      if (left === 0) for (const run of this.#thawed.splice(0)) run();
    };
  }

  /** Runs `run` once nothing holds, now when nothing does */
  whenThawed(run: () => void) {
    if (this.frozen) this.#thawed.push(run);
    else run();
  }
}

export const interaction = new Interaction();

/**
 * Holds the freeze while the pointer is pressed in `node` or a selection that isn't collapsed
 * lies in it. For `{@attach}` on a list's element
 */
export const freezesWhileUsed = (node: HTMLElement) => {
  let pressed: (() => void) | null = null;
  let selecting: (() => void) | null = null;
  const release = () => {
    pressed?.();
    pressed = null;
  };
  const down = () => {
    release();
    pressed = interaction.hold();
  };
  const selection = () => {
    const sel = document.getSelection();
    const inside =
      !!sel &&
      !sel.isCollapsed &&
      sel.rangeCount > 0 &&
      node.contains(sel.getRangeAt(0).commonAncestorContainer);
    if (inside && !selecting) selecting = interaction.hold();
    if (!inside && selecting) {
      selecting();
      selecting = null;
    }
  };
  node.addEventListener("pointerdown", down);
  window.addEventListener("pointerup", release);
  window.addEventListener("pointercancel", release);
  document.addEventListener("selectionchange", selection);
  return () => {
    node.removeEventListener("pointerdown", down);
    window.removeEventListener("pointerup", release);
    window.removeEventListener("pointercancel", release);
    document.removeEventListener("selectionchange", selection);
    release();
    selecting?.();
  };
};
