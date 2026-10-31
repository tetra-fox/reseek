import type { UserSummary } from "./users.svelte.js";

/** How close a name is to the typed text, lower is closer; null when it does not hold the text */
const closeness = (name: string, text: string): number | null => {
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
 * The users whose name holds `text` in any case, closest first: the exact name, the name in
 * another case, names starting with the text, then a word in the name starting with it, then
 * the text anywhere. Shorter names go first within each, then the order the users came in
 */
export const bestMatches = (users: UserSummary[], text: string): UserSummary[] =>
  users
    .flatMap((user) => {
      const rank = closeness(user.username, text);
      return rank === null ? [] : [{ user, rank }];
    })
    .toSorted((a, b) => a.rank - b.rank || a.user.username.length - b.user.username.length)
    .map(({ user }) => user);
