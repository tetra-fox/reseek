import { MediaQuery } from "svelte/reactivity";

/** Below Tailwind's `md`: a phone, where panels become sheets over the screen */
const narrow = new MediaQuery("max-width: 767px");

/** What the window's width allows, decided here once for every screen */
export const viewport = {
  get phone() {
    return narrow.current;
  }
};

/**
 * The pixels in a rem, read when called: a virtual list places its lines in pixels and sizes
 * them in rem with this, so a larger text size gets taller lines
 */
export const remPx = (): number => parseFloat(getComputedStyle(document.documentElement).fontSize);
