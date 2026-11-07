import { createSubscriber, MediaQuery } from "svelte/reactivity";

/** Below Tailwind's `md`: a phone, where panels become sheets over the screen */
const narrow = new MediaQuery("max-width: 767px");
const sm = new MediaQuery("min-width: 640px");
const lg = new MediaQuery("min-width: 1024px");

/** What the window's width allows, decided here once for every screen */
export const viewport = {
  get phone() {
    return narrow.current;
  },
  /** at least Tailwind's `sm`, for a label that its classes hide from there on */
  get sm() {
    return sm.current;
  },
  /** at least Tailwind's `lg` */
  get lg() {
    return lg.current;
  }
};

// a probe 1rem tall changes size with the root font size, which nothing else announces
const rootSize = createSubscriber((update) => {
  const probe = document.createElement("div");
  probe.setAttribute("aria-hidden", "true");
  probe.style.cssText =
    "position:absolute;top:0;left:0;width:0;height:1rem;visibility:hidden;pointer-events:none";
  document.body.append(probe);
  const observer = new ResizeObserver(update);
  observer.observe(probe);
  return () => {
    observer.disconnect();
    probe.remove();
  };
});

/**
 * The pixels in a rem, followed as the text size changes: a virtual list places its lines in
 * pixels and sizes them in rem with this, so a larger text size gets taller lines
 */
export const rem = {
  get px(): number {
    rootSize();
    return parseFloat(getComputedStyle(document.documentElement).fontSize);
  }
};
