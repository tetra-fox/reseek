import { on } from "svelte/events";

const FOCUSABLE = "a[href],button,input,select,textarea,summary,[tabindex]";

/**
 * For `{@attach}` on a copy of controls the keys reach elsewhere, one hidden from assistive
 * technology: its controls still answer the pointer but stay out of the tab order, as they come
 * and go, and focus that comes to one anyway, from a press or a closing menu, goes on to `refocus`
 */
export const untabbable = (refocus: () => void) => (node: HTMLElement) => {
  const take = () => {
    for (const control of node.querySelectorAll<HTMLElement>(FOCUSABLE)) {
      if (control.tabIndex !== -1) control.tabIndex = -1;
    }
  };
  take();
  const watch = new MutationObserver(take);
  watch.observe(node, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["tabindex"]
  });
  const stop = on(node, "focusin", refocus);
  return () => {
    watch.disconnect();
    stop();
  };
};
