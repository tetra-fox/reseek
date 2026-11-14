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
 * Shift+/ on most keyboards. With `altGr` the chord is also pressed while AltGr is held, whatever
 * Ctrl and Alt say then: a layout types some characters with AltGr, which Windows reports as
 * Ctrl+Alt. Escape is pressed whatever is held with it
 */
export type Chord = { key: string; mod?: boolean; alt?: boolean; shift?: boolean; altGr?: boolean };

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

const chordPressed = (c: Chord, e: KeyboardEvent) => {
  if (!sameKey(c.key, e.key)) return false;
  if (c.key === "Escape") return true;
  const held =
    (!!c.altGr && e.getModifierState("AltGraph")) ||
    (!!c.mod === (e.ctrlKey || e.metaKey) && !!c.alt === e.altKey);
  return held && (c.shift === undefined || c.shift === e.shiftKey);
};

/** Whether `e` is one of the binding's keys */
export const isPressed = (binding: Binding, e: KeyboardEvent) =>
  binding.test ? binding.test(e) : binding.keys.some((c) => chordPressed(c, e));

/** For a binding of two opposite keys: -1 for the first, 1 for the second, 0 for any other */
export const keyStep = (binding: Binding, e: KeyboardEvent): -1 | 0 | 1 => {
  const [back, forward] = binding.keys;
  if (back && chordPressed(back, e)) return -1;
  return forward && chordPressed(forward, e) ? 1 : 0;
};

/** Where a key's name is used: on its key cap, in `aria-keyshortcuts`, and read aloud */
type Naming = "cap" | "aria" | "spoken";

/**
 * The keys held with another, in the order a chord names them, with what each naming calls
 * them on a Mac and elsewhere
 */
const HELD: [held: "mod" | "alt" | "shift", names: Record<Naming, [mac: string, other: string]>][] =
  [
    ["mod", { cap: ["⌘", "Ctrl"], aria: ["Meta", "Control"], spoken: ["Command", "Control"] }],
    ["alt", { cap: ["⌥", "Alt"], aria: ["Alt", "Alt"], spoken: ["Option", "Alt"] }],
    ["shift", { cap: ["⇧", "Shift"], aria: ["Shift", "Shift"], spoken: ["Shift", "Shift"] }]
  ];

/** What each naming calls a key, where it isn't the key's own name */
const KEY_NAMES: Record<string, Partial<Record<Naming, string>>> = {
  "ArrowUp": { cap: "↑", spoken: "Up" },
  "ArrowDown": { cap: "↓", spoken: "Down" },
  "ArrowLeft": { cap: "←", spoken: "Left" },
  "ArrowRight": { cap: "→", spoken: "Right" },
  " ": { cap: "Space", aria: "Space", spoken: "Space" },
  "Escape": { cap: "Esc" },
  "PageUp": { cap: "Page Up", spoken: "Page Up" },
  "PageDown": { cap: "Page Down", spoken: "Page Down" },
  "ContextMenu": { cap: "Menu", spoken: "Menu" }
};

/** A chord's names as one naming gives them, in order: what is held, then the key */
const named = (c: Chord, naming: Naming, onMac: boolean): string[] => [
  ...HELD.filter(([held]) => c[held]).map(([, names]) => names[naming][onMac ? 0 : 1]),
  KEY_NAMES[c.key]?.[naming] ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key)
];

/**
 * A chord as the key caps pressed for it, in order: the Command sign and "K" on a Mac, "Ctrl"
 * and "K" elsewhere
 */
export const capsOf = (c: Chord, onMac = mac): string[] => named(c, "cap", onMac);

/** A binding's keys as text for a tooltip or a hint: "Ctrl+K", or the Command sign and K on a Mac */
export const keysText = (binding: Binding, onMac = mac) =>
  binding.keys.map((c) => capsOf(c, onMac).join(onMac ? "" : "+")).join(" or ");

/** A binding's keys for `aria-keyshortcuts`: "Control+K", or "Meta+K" on a Mac */
export const keysAria = (binding: Binding, onMac = mac) =>
  binding.keys.map((c) => named(c, "aria", onMac).join("+")).join(" ");

/**
 * A binding's keys in words, for text a screen reader reads: "Alt+Shift+Up or Down", or
 * "Option+Shift+Up or Down" on a Mac. Keys held with every chord are named once
 */
export const keysSpoken = (binding: Binding, onMac = mac) => {
  const chords = binding.keys.map((c) => named(c, "spoken", onMac));
  const held = (names: string[]) => names.slice(0, -1).join("+");
  const shared = chords.every((names) => held(names) === held(chords[0]));
  return chords.map((names, i) => (shared && i ? names.at(-1) : names.join("+"))).join(" or ");
};
