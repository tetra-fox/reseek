import { chat } from "#lib/chat/chat.svelte.js";
import type { Target } from "#lib/chat/pane.svelte.js";
import { users } from "#lib/users/users.svelte.js";

const time = (iso: string | null | undefined) => (iso ? Date.parse(iso) : 0);

/** The joined rooms, the latest message first */
export const sortedRooms = () =>
  chat.rooms.rows.toSorted(
    (a, b) => time(b.lastMessageAt) - time(a.lastMessageAt) || a.name.localeCompare(b.name)
  );

/**
 * The open conversations: pinned first, then the newest message first. Local ids grow in the
 * order messages were stored, so a replayed message dated long ago still counts as new
 */
export const sortedConversations = () =>
  chat.conversations.rows.toSorted(
    (a, b) =>
      Number(!!users.get(b.username)?.pinned) - Number(!!users.get(a.username)?.pinned) ||
      (b.lastMessage?.localId ?? 0) - (a.lastMessage?.localId ?? 0) ||
      a.username.localeCompare(b.username)
  );

/** Every chat in the order the list shows them, rooms then conversations, for moving between them */
export const chatOrder = (): Target[] => [
  ...sortedRooms().map((r): Target => ({ kind: "room", name: r.name })),
  ...sortedConversations().map((c): Target => ({ kind: "user", name: c.username }))
];
