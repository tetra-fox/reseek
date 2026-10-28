/**
 * Whether a key comes with Ctrl, Cmd or Alt, which makes it the browser's or the system's
 * shortcut (copy, reload, back) rather than one of the page's single keys
 */
export const withModifier = (e: KeyboardEvent) => e.ctrlKey || e.metaKey || e.altKey;

/** Elements that take every key: text fields, and menus and dialogs with their own keys */
const KEEPS_ALL =
  "input,textarea,select,[contenteditable],[role=menu],[role=menuitem],[role=menuitemradio],[role=menuitemcheckbox],[role=dialog],[role=alertdialog]";
/** Elements that take Enter and Space and leave the other keys to what they are in */
const KEEPS_PRESS = "button,a[href],[role=checkbox],[role=switch],summary";

/**
 * Whether the event's target is in an element matching `selector` that lies inside the element
 * handling it, so a list inside a dialog or a menu is not taken for one of its controls
 */
const inside = (e: Event, selector: string) => {
  const target = e.target;
  if (!(target instanceof Element)) return false;
  const hit = target.closest(selector);
  const handler = e.currentTarget instanceof Element ? e.currentTarget : null;
  return !!hit && (!handler || (hit !== handler && handler.contains(hit)));
};

/**
 * Whether a key belongs to the element it was pressed on, so a list's own keys leave it
 * alone: one something already handled, every key in a text field, a menu or a dialog, and
 * Enter and Space on a button, checkbox or link
 */
export const keepsKey = (e: KeyboardEvent): boolean => {
  if (e.defaultPrevented) return true;
  if (inside(e, KEEPS_ALL)) return true;
  return inside(e, KEEPS_PRESS) && (e.key === "Enter" || e.key === " ");
};

/** Whether a click belongs to a control: a field, a button, a link, a checkbox */
export const inControl = (e: MouseEvent) => inside(e, `${KEEPS_ALL},${KEEPS_PRESS},label`);

/**
 * Whether a menu, a dialog or a hover card is open, which Escape closes before anything under
 * it. One closing stays in the page for its exit animation, marked `data-state="closed"`, and
 * no longer counts
 */
export const layerOpen = () =>
  !!document.querySelector(
    ["menu", "dialog", "alertdialog"].map((r) => `[role=${r}]:not([data-state=closed])`).join(",")
  );

/** Whether the keyboard is a Mac's, whose shortcuts take Cmd where others take Ctrl */
export const mac = typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform);

/** Ctrl+J, or Cmd+J on a Mac: opens or closes the activity log from anywhere */
export const togglesActivity = (e: KeyboardEvent) =>
  (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "j";

/** A Ctrl/Cmd shortcut as the keyboard in use names it: "Ctrl+K", or "⌘K" on a Mac */
export const shortcutText = (key: string) => (mac ? `⌘${key}` : `Ctrl+${key}`);

/** A Ctrl/Cmd shortcut for `aria-keyshortcuts`: "Control+K", or "Meta+K" on a Mac */
export const shortcutAria = (key: string) => `${mac ? "Meta" : "Control"}+${key}`;

/**
 * Ctrl+K, or Cmd+K on a Mac: opens the command palette from anywhere, a text field included,
 * since no field takes it for itself
 */
export const opensPalette = (e: KeyboardEvent) =>
  (e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k";
