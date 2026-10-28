import { getContext, setContext } from "svelte";

const KEY = Symbol("menu anchor");

/**
 * Sets where a menu opened by `LazyRoot` hangs: the button that opened it, which its content
 * anchors to and gives focus back to when it closes
 */
export const setAnchor = (anchor: () => HTMLElement | null) => setContext(KEY, anchor);

/** The anchor a `LazyRoot` around this content set, undefined in a menu with its own trigger */
export const anchorOf = () => getContext<(() => HTMLElement | null) | undefined>(KEY);
