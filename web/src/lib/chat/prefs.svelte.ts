import { fields, stored } from "#lib/stored.svelte.js";

/** The unsent text of each room's composer, kept in this browser */
export const roomDrafts = stored("reseek.chat.roomDrafts", (saved) => {
  const drafts: Record<string, string> = {};
  for (const [room, text] of Object.entries(fields(saved))) {
    if (typeof text === "string" && text) drafts[room] = text;
  }
  return drafts;
});

/**
 * How the chat page is laid out in this browser: whether a room's side panel shows and which of
 * its two (the members or the wall), whether `:name` offers emoji, and the rooms whose joins,
 * leaves and presence lines are hidden
 */
export const chatLayout = stored("reseek.chat.layout", (saved) => {
  const s = fields(saved);
  return {
    users: s.users !== false,
    side: s.side === "wall" ? ("wall" as const) : ("users" as const),
    emoji: s.emoji !== false,
    quiet: Array.isArray(s.quiet) ? s.quiet.filter((r) => typeof r === "string") : ([] as string[])
  };
});

/** Whether a room shows the lines for members joining, leaving, going away and coming back */
export const showsPresence = (room: string) => !chatLayout.quiet.includes(room);

/** Turns a room's presence lines on or off */
export const setPresence = (room: string, on: boolean) => {
  chatLayout.quiet = on
    ? chatLayout.quiet.filter((r) => r !== room)
    : [...chatLayout.quiet.filter((r) => r !== room), room];
};
