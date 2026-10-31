import type { Schemas } from "#lib/api/client.js";
import { paneKey } from "#lib/chat/chat.svelte.js";
import type { Target } from "#lib/chat/pane.svelte.js";
import { bestMatches, closeness, type UserFound } from "#lib/users/match.js";

export type MessageHit = Schemas["MessageHit"];
export type SearchKind = Schemas["MessageSearchKind"];

/** A room found by name, `userCount` as the server gave it, null for one it did not list */
export type RoomFound = { name: string; joined: boolean; userCount: number | null };

/** What a search found, the closest first */
export type Found = { rooms: RoomFound[]; users: UserFound[]; hits: MessageHit[] };

/** The one chat an exact name opens */
export type HardResult = { kind: "room"; room: RoomFound } | { kind: "user"; user: UserFound };

/** How much text the box takes: quoting every word of it stays within what the daemon reads */
export const MAX_TEXT = 100;

/** How many entries of each category the dropdown lists */
export const LISTED = 4;

/** What makes a text a written full-text query rather than plain words */
const WRITTEN = /"|\*|\b(?:AND|OR|NOT)\b/;

/**
 * The text as plain words for a full-text query: each one quoted, so nothing in it reads as
 * syntax, all of them required, and the last one open, so a word being typed matches
 */
export const plainQuery = (text: string) => {
  const words = text.replaceAll('"', " ").split(/\s+/).filter(Boolean);
  return words.length ? `${words.map((w) => `"${w}"`).join(" ")}*` : "";
};

/** The full-text query for what was typed: the text itself when it is written as one */
export const fullTextQuery = (text: string) => (WRITTEN.test(text) ? text : plainQuery(text));

/**
 * The rooms and users whose names hold `text`, closest first. A name given twice counts once,
 * with what was given for it last
 */
export const matchNames = (
  rooms: RoomFound[],
  users: UserFound[],
  text: string
): Pick<Found, "rooms" | "users"> => ({
  rooms: bestMatches([...new Map(rooms.map((r) => [r.name, r])).values()], text, (r) => r.name),
  users: bestMatches([...new Map(users.map((u) => [u.name, u])).values()], text, (u) => u.name)
});

/** A message's place among the hits: its text compared as a name, one without the whole text last */
const place = (hit: MessageHit, text: string) => closeness(hit.message, text) ?? 5;

/** The hits that hold the text most plainly first, the newest first within each */
export const rankHits = (hits: MessageHit[], text: string): MessageHit[] =>
  hits.toSorted((a, b) => place(a, text) - place(b, text));

/** The one room or user named exactly `text`, null when there is none or both */
export const hardResult = ({ rooms, users }: Found, text: string): HardResult | null => {
  const room = rooms.find((r) => r.name === text);
  const user = users.find((u) => u.name === text);
  if (room && user) return null;
  if (room) return { kind: "room", room };
  return user ? { kind: "user", user } : null;
};

/** The chat a hit is in */
export const hitTarget = (hit: MessageHit): Target =>
  hit.room ? { kind: "room", name: hit.room } : { kind: "user", name: hit.username };

/** The chats the hits are in, each with its hits, in the order the hits came */
export const groupHits = (hits: MessageHit[]) => {
  const groups = new Map<string, { target: Target; hits: MessageHit[] }>();
  for (const hit of hits) {
    const target = hitTarget(hit);
    const key = paneKey(target.kind, target.name);
    const group = groups.get(key) ?? { target, hits: [] };
    group.hits.push(hit);
    groups.set(key, group);
  }
  return [...groups.entries()].map(([key, group]) => ({ key, ...group }));
};

/**
 * What the dropdown lists: the hard result apart, ahead of everything, and at most `LISTED` of
 * each category without it. The hits are the best few
 */
export const dropdown = (found: Found, text: string) => {
  const hard = hardResult(found, text);
  const rooms = found.rooms.filter((r) => !(hard?.kind === "room" && hard.room.name === r.name));
  const users = found.users.filter((u) => !(hard?.kind === "user" && hard.user.name === u.name));
  return {
    hard,
    rooms: rooms.slice(0, LISTED),
    users: users.slice(0, LISTED),
    hits: rankHits(found.hits, text).slice(0, LISTED)
  };
};
