import { on } from "svelte/events";
import { SvelteMap } from "svelte/reactivity";

/** Where a toolbar is, in pixels up from the content area's bottom edge */
type Span = { from: number; to: number };

/**
 * The toolbars in the content area that the toasts keep off, such as a selection's, which rests at
 * the content area's foot. A toolbar outside the content area, as in a dialog, is not counted
 */
class FootBars {
  #bars = new SvelteMap<Element, Span>();

  /**
   * How far above the content area's bottom a stack of toasts `height` tall rests: `gap` above
   * it, or `gap` above a toolbar it would lie on there
   */
  rest(gap: number, height: number): number {
    let at = gap;
    for (const { from, to } of this.#bars.values()) {
      if (from < at + height && to > at) at = Math.max(at, to + gap);
    }
    return at;
  }

  /** For `{@attach}` on such a toolbar */
  attach = (node: HTMLElement) => {
    const content = node.closest<HTMLElement>("[data-content]");
    if (!content) return;
    const measure = () => {
      const bottom = content.getBoundingClientRect().bottom;
      const box = node.getBoundingClientRect();
      const span = { from: bottom - box.bottom, to: bottom - box.top };
      const was = this.#bars.get(node);
      if (was?.from !== span.from || was.to !== span.to) this.#bars.set(node, span);
    };
    // it moves as the content scrolls, as the page outgrows the content area or shrinks into it,
    // and as the window resizes
    const watch = new ResizeObserver(measure);
    for (const target of [node, content, ...content.children]) watch.observe(target);
    const stop = on(content, "scroll", measure, { passive: true });
    measure();
    return () => {
      watch.disconnect();
      stop();
      this.#bars.delete(node);
    };
  };
}

export const footBars = new FootBars();
