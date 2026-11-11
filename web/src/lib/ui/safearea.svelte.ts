import { on } from "svelte/events";

type Insets = { top: number; right: number; bottom: number; left: number };

/**
 * The safe area's insets in pixels, read from the page's --safe-* properties and read again as
 * the window resizes or the phone turns: what a notch, rounded corners or a browser bar cover
 */
class SafeArea {
  #insets = $state.raw<Insets>({ top: 0, right: 0, bottom: 0, left: 0 });

  constructor() {
    if (typeof window === "undefined") return;
    this.#measure();
    on(window, "resize", this.#measure);
  }

  /** A popover's collision padding: `gap` from the screen's edges, inside the safe area */
  padding(gap: number): Insets {
    const { top, right, bottom, left } = this.#insets;
    return { top: top + gap, right: right + gap, bottom: bottom + gap, left: left + gap };
  }

  #measure = () => {
    const probe = document.createElement("div");
    probe.style.cssText =
      "position:fixed;visibility:hidden;padding:var(--safe-t) var(--safe-r) var(--safe-b) var(--safe-l)";
    document.body.append(probe);
    const style = getComputedStyle(probe);
    // a browser without env() leaves the padding empty, which reads as no inset
    const px = (value: string) => parseFloat(value) || 0;
    this.#insets = {
      top: px(style.paddingTop),
      right: px(style.paddingRight),
      bottom: px(style.paddingBottom),
      left: px(style.paddingLeft)
    };
    probe.remove();
  };
}

export const safeArea = new SafeArea();
