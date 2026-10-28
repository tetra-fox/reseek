import type { Target } from "./pane.svelte";

/**
 * What a chat route shows, from its path as written: `/chat/search` the message search,
 * `/chat/rooms/<room>` a room, `/chat/<user>` a conversation, `/chat` none. The names are read
 * from the written path, so a user called `search` is reached as `/chat/%73earch`. A path outside
 * `/chat` shows none: the chat screen still reads the address for a moment after leaving it
 */
export const chatTarget = (path: string): Target | "search" | null => {
  const [, root, ...parts] = path.split("/");
  if (root !== "chat" || !parts.length || !parts[0]) return null;
  if (parts.length === 1 && parts[0] === "search") return "search";
  if (parts[0] === "rooms" && parts.length === 2 && parts[1]) {
    return { kind: "room", name: decodeURIComponent(parts[1]) };
  }
  if (parts.length === 1) return { kind: "user", name: decodeURIComponent(parts[0]) };
  return null;
};
