import { MediaQuery } from "svelte/reactivity";

/** Below Tailwind's `md`: a phone, where panels become sheets over the screen */
const narrow = new MediaQuery("max-width: 767px");

/** What the window's width allows, decided here once for every screen */
export const viewport = {
  get phone() {
    return narrow.current;
  }
};
