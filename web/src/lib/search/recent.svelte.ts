import { fields, stored } from "#lib/stored.svelte.js";

/** How many recent searches are kept */
export const RECENT_LIMIT = 12;

const texts = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((t): t is string => typeof t === "string" && !!t.trim()).slice(0, RECENT_LIMIT)
    : [];

/**
 * The searches typed in this browser, newest first, and the ones pinned to stay under the search
 * box. A pinned search stays out of the recent list
 */
export const recent = stored("reseek.search.recent", (saved) => {
  const s = fields(saved);
  return { texts: texts(s.texts), pinned: texts(s.pinned) };
});

/** Puts a search at the top of the recent ones, once */
export const remember = (text: string) => {
  const typed = text.trim();
  if (!typed || recent.pinned.includes(typed)) return;
  recent.texts = [typed, ...recent.texts.filter((t) => t !== typed)].slice(0, RECENT_LIMIT);
};

/** Takes a search off the recent ones; gives back where it was, for an undo */
export const forget = (text: string): number => {
  const at = recent.texts.indexOf(text);
  if (at !== -1) recent.texts.splice(at, 1);
  return at;
};

/** Puts a search back where it was taken off */
export const restore = (text: string, at: number) => {
  if (at === -1 || recent.texts.includes(text)) return;
  recent.texts.splice(Math.min(at, recent.texts.length), 0, text);
};

/** Pins a search under the box, or unpins it back to the recent ones */
export const togglePin = (text: string) => {
  if (recent.pinned.includes(text)) {
    recent.pinned = recent.pinned.filter((t) => t !== text);
    remember(text);
  } else {
    recent.pinned = [...recent.pinned, text];
    forget(text);
  }
};
