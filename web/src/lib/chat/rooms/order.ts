import { chat } from "#lib/chat/chat.svelte.js";
import { NOTICES } from "#lib/chat/notices.js";
import type { Target } from "#lib/chat/pane.svelte.js";
import { pins } from "#lib/chat/pins.svelte.js";

const time = (iso: string | null | undefined) => (iso ? Date.parse(iso) : 0);

/**
 * The pinned chats the list shows, in the user's order. A pin of a room left or a conversation
 * closed is kept, and the chat is back in its place when it is back
 */
export const pinnedChats = (): Target[] =>
  pins.list.filter((pin) =>
    pin.kind === "room" ? chat.rooms.get(pin.name) : chat.conversations.get(pin.name)
  );

/** The joined rooms not pinned, the latest message first */
export const sortedRooms = () =>
  chat.rooms.rows
    .filter((r) => !pins.has({ kind: "room", name: r.name }))
    .toSorted(
      (a, b) => time(b.lastMessageAt) - time(a.lastMessageAt) || a.name.localeCompare(b.name)
    );

/**
 * The open conversations not pinned, the newest message first. Local ids grow in the order
 * messages were stored, so a replayed message dated long ago still counts as new
 */
export const sortedConversations = () =>
  chat.conversations.rows
    .filter((c) => !pins.has({ kind: "user", name: c.username }))
    .toSorted(
      (a, b) =>
        (b.lastMessage?.localId ?? 0) - (a.lastMessage?.localId ?? 0) ||
        a.username.localeCompare(b.username)
    );

/** Every chat in the order the list shows them, the server's notices last, for moving between them */
export const chatOrder = (): Target[] => [
  ...pinnedChats(),
  ...sortedRooms().map((r): Target => ({ kind: "room", name: r.name })),
  ...sortedConversations().map((c): Target => ({ kind: "user", name: c.username })),
  ...(chat.latestNotice ? [NOTICES] : [])
];
