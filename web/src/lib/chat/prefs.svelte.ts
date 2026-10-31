import { fields, stored } from "#lib/stored.svelte.js";

/** The unsent text of each room's composer, kept in this browser */
export const roomDrafts = stored("reseek.chat.roomDrafts", (saved) => {
  const drafts: Record<string, string> = {};
  for (const [room, text] of Object.entries(fields(saved))) {
    if (typeof text === "string" && text) drafts[room] = text;
  }
  return drafts;
});

/** The tabs of a room's side panel */
export const SIDE_TABS = [
  { id: "users", title: "Members" },
  { id: "activity", title: "Activity" },
  { id: "wall", title: "Wall" }
] as const;

export type SideTab = (typeof SIDE_TABS)[number]["id"];

/** A column's width as `saved` holds it, null while it has not been dragged */
const width = (saved: unknown) => (typeof saved === "number" && saved > 0 ? saved : null);

/**
 * How the chat page is laid out in this browser: whether a room's side panel shows on a wide
 * screen, which of its tabs is open (one choice for every room, as a way of working rather than
 * a fact about a room), the widths the chat list and the side panel were dragged to, and whether
 * `:name` offers emoji
 */
export const chatLayout = stored("reseek.chat.layout", (saved) => {
  const s = fields(saved);
  return {
    users: s.users !== false,
    side: (SIDE_TABS.find((t) => t.id === s.side)?.id ?? "users") as SideTab,
    listWidth: width(s.listWidth),
    sideWidth: width(s.sideWidth),
    emoji: s.emoji !== false
  };
});
