import type { SnippetPart } from "#lib/text.js";

import type { UserSummary } from "./users.svelte";

/** A user found by name, `summary` when the directory answered for them */
export type UserFound = { name: string; summary?: UserSummary };

/** How close a name is to the typed text, lower is closer; null when it does not hold the text */
export const closeness = (name: string, text: string): number | null => {
  if (name === text) return 0;
  const lower = name.toLowerCase();
  const wanted = text.toLowerCase();
  if (lower === wanted) return 1;
  if (lower.startsWith(wanted)) return 2;
  let found = false;
  for (let at = lower.indexOf(wanted); at >= 0; at = lower.indexOf(wanted, at + 1)) {
    found = true;
    if (!/[a-z0-9]/.test(lower.charAt(at - 1))) return 3;
  }
  return found ? 4 : null;
};

/**
 * The items whose name holds `text` in any case, closest first: the exact name, the name in
 * another case, names starting with the text, then a word in the name starting with it, then
 * the text anywhere. Shorter names go first within each, then the order the items came in
 */
export const bestMatches = <T>(items: T[], text: string, name: (item: T) => string): T[] =>
  items
    .flatMap((item) => {
      const rank = closeness(name(item), text);
      return rank === null ? [] : [{ item, rank }];
    })
    .toSorted((a, b) => a.rank - b.rank || name(a.item).length - name(b.item).length)
    .map(({ item }) => item);

/** `name` as runs of text, the first place it holds `text`, in any case, marked as a hit */
export const nameParts = (name: string, text: string): SnippetPart[] => {
  const at = name.toLowerCase().indexOf(text.toLowerCase());
  if (at === -1 || !text) return [{ at: 0, text: name, hit: false }];
  const end = at + text.length;
  return [
    { at: 0, text: name.slice(0, at), hit: false },
    { at, text: name.slice(at, end), hit: true },
    { at: end, text: name.slice(end), hit: false }
  ].filter((part) => part.text);
};
