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
 * Whether a key belongs to the element it was pressed on, so a list's own keys leave it
 * alone: one something already handled, every key in a text field, a menu or a dialog, and
 * Enter and Space on a button, checkbox or link
 */
export const keepsKey = (e: KeyboardEvent): boolean => {
  if (e.defaultPrevented) return true;
  const target = e.target;
  if (!(target instanceof Element)) return false;
  if (target.closest(KEEPS_ALL)) return true;
  return !!target.closest(KEEPS_PRESS) && (e.key === "Enter" || e.key === " ");
};

/**
 * Whether a menu, a dialog or a hover card is open, which Escape closes before anything under
 * it. They are only in the page while open
 */
export const layerOpen = () =>
  !!document.querySelector("[role=menu],[role=dialog],[role=alertdialog]");
