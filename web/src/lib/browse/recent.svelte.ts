import { fields, stored } from "#lib/stored.svelte.js";

/** How many browsed users are remembered */
export const RECENT_USERS = 10;

/** The users browsed in this browser, the latest first, listed under the search box */
export const browsed = stored("reseek.browse.recent", (saved) => {
  const users = fields(saved).users;
  return {
    users: Array.isArray(users)
      ? users.filter((u): u is string => typeof u === "string" && !!u).slice(0, RECENT_USERS)
      : []
  };
});

/** Puts a user at the top of the browsed ones, once */
export const rememberBrowsed = (username: string) => {
  browsed.users = [username, ...browsed.users.filter((u) => u !== username)].slice(0, RECENT_USERS);
};

/** Takes a user off; gives back where they were, for an undo */
export const forgetBrowsed = (username: string): number => {
  const at = browsed.users.indexOf(username);
  if (at !== -1) browsed.users.splice(at, 1);
  return at;
};

/** Puts a user back where they were taken off */
export const restoreBrowsed = (username: string, at: number) => {
  if (at === -1 || browsed.users.includes(username)) return;
  browsed.users.splice(Math.min(at, browsed.users.length), 0, username);
};
