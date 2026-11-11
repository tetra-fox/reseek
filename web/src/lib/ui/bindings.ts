import type { Binding, Chord } from "./keys";

/**
 * The keys of one place in the app, under the title the shortcuts sheet gives them. `here` says
 * whether a route is that place, for a group that belongs to one screen
 */
export type BindingGroup = { title: string; bindings: Binding[]; here?: (path: string) => boolean };

const groups: BindingGroup[] = [];

/** Declares the keys of one place; each group declared here is listed on the shortcuts sheet */
const group = <T extends Record<string, Binding>>(
  title: string,
  bindings: T,
  here?: (path: string) => boolean
): T => {
  groups.push({ title, bindings: Object.values(bindings), here });
  return bindings;
};

const inChat = (path: string) => path === "/chat" || path.startsWith("/chat/");

const bind = (label: string, ...keys: Chord[]): Binding => ({ label, keys });
const key = (name: string, held: Omit<Chord, "key"> = {}): Chord => ({ key: name, ...held });

const UP = key("ArrowUp");
const DOWN = key("ArrowDown");
const LEFT = key("ArrowLeft");
const RIGHT = key("ArrowRight");
const SPACE = key(" ");
const ENTER = key("Enter");
const ESCAPE = key("Escape");
const MENU = [key("ContextMenu"), key("F10", { shift: true })];

export const APP = group("Everywhere", {
  // no text field takes it for itself, so it works from one too
  palette: bind("Open command palette", key("k", { mod: true, shift: false })),
  activity: bind("Show or hide activity log", key("j", { mod: true, shift: false })),
  shortcuts: bind("Show keyboard shortcuts", key("?"))
});

export const LIST = group("Lists and trees", {
  move: bind("Previous or next row", UP, DOWN),
  ends: bind("First or last row", key("Home"), key("End")),
  page: bind("Page up or down", key("PageUp"), key("PageDown")),
  select: bind("Select row", SPACE),
  selectAll: bind("Select all", key("a", { mod: true })),
  open: bind("Open row", ENTER),
  clear: bind("Clear selection", ESCAPE),
  // one character of a name, not a space, with nothing but Shift held
  find: {
    label: "Go to row by name",
    keys: [],
    typed: "Type the name",
    test: (e) => e.key.length === 1 && e.key !== " " && !e.ctrlKey && !e.metaKey && !e.altKey
  },
  menu: bind("Open row menu", ...MENU),
  folder: bind("Close or open folder", LEFT, RIGHT),
  folderAll: bind(
    "Close or open folder and every folder inside",
    key("ArrowLeft", { mod: true }),
    key("ArrowRight", { mod: true })
  ),
  cancelDrag: bind("Cancel drag", ESCAPE)
});

export const DETAILS = group("Lists with details", {
  details: bind("Show or hide details", key(" ", { shift: false }), key("i", { shift: false })),
  select: bind("Select row", key(" ", { shift: true }))
});

export const TRANSFERS = group(
  "Transfers",
  {
    remove: bind("Remove", key("Delete")),
    retry: bind("Retry or download again", key("r")),
    cancel: bind("Cancel", key("c")),
    open: bind("Open file", key("o"))
  },
  (path) => path === "/downloads" || path === "/uploads"
);

export const FILES = group(
  "Files",
  {
    remove: bind("Delete", key("Delete"))
  },
  (path) => path === "/system/files"
);

export const CHAT = group(
  "Chat",
  {
    next: bind(
      "Previous or next chat",
      key("ArrowUp", { alt: true, shift: false }),
      key("ArrowDown", { alt: true, shift: false })
    ),
    movePin: bind(
      "Move pinned chat up or down",
      key("ArrowUp", { alt: true, shift: true }),
      key("ArrowDown", { alt: true, shift: true })
    ),
    backToResults: bind("Back to search results", ESCAPE),
    join: bind("Join room in focus, from the room filter", ENTER)
  },
  inChat
);

export const COMPOSER = group(
  "Message box",
  {
    // Ctrl or Cmd+Enter, the send key of many chat apps, and Alt+Enter send as Enter does
    send: bind(
      "Send",
      key("Enter", { shift: false }),
      key("Enter", { mod: true, shift: false }),
      key("Enter", { alt: true, shift: false })
    ),
    newLine: bind("New line, in a room", key("Enter", { shift: true })),
    recall: bind("Edit last message sent", key("ArrowUp", { shift: false })),
    complete: bind("Complete name or command", key("Tab")),
    choose: bind("Previous or next suggestion", UP, DOWN),
    hide: bind("Hide command reply", ESCAPE)
  },
  inChat
);

export const MESSAGES = group(
  "Messages",
  {
    move: bind("Previous or next message", UP, DOWN),
    menu: bind("Open message menu", ENTER, ...MENU),
    actions: bind(
      "Leave or go to message actions",
      key("Tab", { shift: true }),
      key("Tab", { shift: false })
    ),
    back: bind("Back to message", ESCAPE),
    leave: bind("Leave messages", ESCAPE)
  },
  inChat
);

export const SUGGESTIONS = group("Boxes with suggestions", {
  take: bind("Take suggestion", ENTER)
});

export const USERNAMES = group("Usernames", {
  card: bind("Show user card", SPACE, key("ArrowDown", { alt: true })),
  menu: bind("Open user menu", ...MENU)
});

export const ANALYSIS = group(
  "Analysis",
  {
    cursor: bind(
      "Previous or next window",
      key("ArrowLeft", { shift: false }),
      key("ArrowRight", { shift: false })
    ),
    leap: bind(
      "Ten windows back or forward",
      key("ArrowLeft", { shift: true }),
      key("ArrowRight", { shift: true })
    ),
    ends: bind("First or last window", key("Home"), key("End")),
    zoomIn: bind("Zoom in", key("+"), key("=")),
    zoomOut: bind("Zoom out", key("-")),
    play: bind("Play or pause", SPACE)
  },
  (path) => path === "/analysis"
);

export const PANELS = group("Panel edges", {
  side: bind("Resize side panel", LEFT, RIGHT),
  below: bind("Resize activity log", UP, DOWN),
  ends: bind("Smallest or largest size", key("Home"), key("End"))
});

/** Every group of keys, in the order the shortcuts sheet lists them */
export const SHEET: readonly BindingGroup[] = groups;
