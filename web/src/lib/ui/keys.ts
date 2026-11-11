/** How long after the last key typing counts as paused, for a box that reads what was typed */
export const TYPING_PAUSE = 200;

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
 * Whether a menu, a dialog, a hover card or a select's list is open, which Escape closes before
 * anything under it. One closing stays in the page for its exit animation, marked
 * `data-state="closed"`, and no longer counts. A list of options that is part of the page, with
 * no `data-state`, is not a layer
 */
export const layerOpen = () =>
  !!document.querySelector(
    [
      ...["menu", "dialog", "alertdialog"].map((r) => `[role=${r}]:not([data-state=closed])`),
      "[role=listbox][data-state=open]"
    ].join(",")
  );

/** Whether the keyboard is a Mac's, whose shortcuts take Cmd where others take Ctrl */
export const mac = typeof navigator !== "undefined" && /mac|iphone|ipad/i.test(navigator.platform);

/**
 * A key and what is held with it. `mod` is Ctrl or Cmd, either on any keyboard, and shown as
 * the one the keyboard uses. `mod` and `alt` are held exactly as given. Shift is held as given
 * when `shift` is set and either way when it is not, since many keys are typed with it: `?` is
 * Shift+/ on most keyboards
 */
export type Chord = { key: string; mod?: boolean; alt?: boolean; shift?: boolean };

/**
 * What a key does: `keys` are the chords that do it. A binding of two keys that go opposite
 * ways lists the one going back or up first, so `keyStep` can tell them apart. Keys that are not
 * one chord, such as a name typed to find a row, are told by `test` and shown as `typed`
 */
export type Binding = {
  label: string;
  keys: Chord[];
  typed?: string;
  test?: (e: KeyboardEvent) => boolean;
};

const sameKey = (want: string, got: string) =>
  want.length === 1 ? want.toLowerCase() === got.toLowerCase() : want === got;

const chordPressed = (c: Chord, e: KeyboardEvent) =>
  sameKey(c.key, e.key) &&
  !!c.mod === (e.ctrlKey || e.metaKey) &&
  !!c.alt === e.altKey &&
  (c.shift === undefined || c.shift === e.shiftKey);

/** Whether `e` is one of the binding's keys */
export const isPressed = (binding: Binding, e: KeyboardEvent) =>
  binding.test ? binding.test(e) : binding.keys.some((c) => chordPressed(c, e));

/** For a binding of two opposite keys: -1 for the first, 1 for the second, 0 for any other */
export const keyStep = (binding: Binding, e: KeyboardEvent): -1 | 0 | 1 => {
  const [back, forward] = binding.keys;
  if (back && chordPressed(back, e)) return -1;
  return forward && chordPressed(forward, e) ? 1 : 0;
};

/** What a key cap says, where it isn't the key's own name */
const CAPS: Record<string, string> = {
  "ArrowUp": "↑",
  "ArrowDown": "↓",
  "ArrowLeft": "←",
  "ArrowRight": "→",
  " ": "Space",
  "Escape": "Esc",
  "PageUp": "Page Up",
  "PageDown": "Page Down",
  "ContextMenu": "Menu"
};

/**
 * A chord as the key caps pressed for it, in order: the Command sign and "K" on a Mac, "Ctrl"
 * and "K" elsewhere
 */
export const capsOf = (c: Chord, onMac = mac): string[] => [
  ...(c.mod ? [onMac ? "⌘" : "Ctrl"] : []),
  ...(c.alt ? [onMac ? "⌥" : "Alt"] : []),
  ...(c.shift ? [onMac ? "⇧" : "Shift"] : []),
  CAPS[c.key] ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key)
];

/** A binding's keys as text for a tooltip or a hint: "Ctrl+K", or the Command sign and K on a Mac */
export const keysText = (binding: Binding, onMac = mac) =>
  binding.keys.map((c) => capsOf(c, onMac).join(onMac ? "" : "+")).join(" or ");

/** A binding's keys for `aria-keyshortcuts`: "Control+K", or "Meta+K" on a Mac */
export const keysAria = (binding: Binding, onMac = mac) =>
  binding.keys
    .map((c) =>
      [
        ...(c.mod ? [onMac ? "Meta" : "Control"] : []),
        ...(c.alt ? ["Alt"] : []),
        ...(c.shift ? ["Shift"] : []),
        c.key === " " ? "Space" : c.key.length === 1 ? c.key.toUpperCase() : c.key
      ].join("+")
    )
    .join(" ");
