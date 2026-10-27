import { fields, stored } from "#lib/stored.svelte.js";

/** The unsent text of each room's composer, kept in this browser */
export const roomDrafts = stored("reseek.chat.roomDrafts", (saved) => {
  const drafts: Record<string, string> = {};
  for (const [room, text] of Object.entries(fields(saved))) {
    if (typeof text === "string" && text) drafts[room] = text;
  }
  return drafts;
});

/** How the chat page is laid out in this browser: whether a room's user list shows */
export const chatLayout = stored("reseek.chat.layout", (saved) => ({
  users: fields(saved).users !== false
}));
