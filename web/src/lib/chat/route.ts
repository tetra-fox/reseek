import { chatHref, href, roomHref } from "#lib/nav.js";

import type { Target } from "./pane.svelte";
import type { SearchKind } from "./search/match";

/** Whether `path` is the chat screen's */
export const isChatPath = (path: string) => path === "/chat" || path.startsWith("/chat/");

/**
 * What a chat route shows, from its path as written: `/chat/search` the search results,
 * `/chat/public` the feed of every public room, `/chat/rooms/<room>` a room, `/chat/<user>` a
 * conversation, `/chat` none. The names are read from the written path, so a user called
 * `search` is reached as `/chat/%73earch`
 */
export const chatTarget = (path: string): Target | "search" | "public" | null => {
  const parts = path.split("/").slice(2);
  if (!parts.length || !parts[0]) return null;
  if (parts.length === 1 && parts[0] === "search") return "search";
  if (parts.length === 1 && parts[0] === "public") return "public";
  if (parts[0] === "rooms" && parts.length === 2 && parts[1]) {
    return { kind: "room", name: decodeURIComponent(parts[1]) };
  }
  if (parts.length === 1) return { kind: "user", name: decodeURIComponent(parts[0]) };
  return null;
};

/** Which messages the address searches: `in=` when it says, else all of them */
export const searchKind = (value: string | null): SearchKind =>
  value === "private" || value === "rooms" ? value : "all";

const searchQuery = (q: string, kind: SearchKind) =>
  `q=${encodeURIComponent(q)}${kind === "all" ? "" : `&in=${kind}`}`;

/** The link to the search results for `q` */
export const resultsHref = (q: string, kind: SearchKind = "all") =>
  href(`/chat/search?${searchQuery(q, kind)}`);

/** The link to a chat opened from the results for `q`, at the message `at` when given */
export const foundHref = (target: Target, q: string, kind: SearchKind, at?: number) => {
  const chat = target.kind === "user" ? chatHref(target.name) : roomHref(target.name);
  return `${chat}?${at === undefined ? "" : `at=${at}&`}${searchQuery(q, kind)}`;
};
