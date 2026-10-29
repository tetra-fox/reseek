import { parseSlsk, type SlskLink } from "#lib/files/slsk.js";

/**
 * A run of a chat message starting at `at`: plain text, a web link, a `slsk://` link, or a
 * word the reader watches for (their name or a highlight word)
 */
export type Rich =
  | { at: number; text: string; kind: "text" | "mark" }
  | { at: number; text: string; kind: "url"; url: string }
  | { at: number; text: string; kind: "slsk"; link: SlskLink };

/** http(s) and slsk links, up to the first space; trailing punctuation stays text */
const LINK = /(?:https?|slsk):\/\/[^\s<>"]+/gi;
const TRAILING = /[.,;:!?)\]}'"]+$/;

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * The watched words as one pattern: each matched case-folded, and only as a whole word, with no
 * letter, digit or `_` right before or after it, so `floyd.` matches `floyd` and `floyds` does
 * not. Null when there is nothing to watch
 */
const watching = (words: string[]): RegExp | null => {
  const wanted = words.map((w) => w.trim()).filter(Boolean);
  if (!wanted.length) return null;
  // longest first, so a phrase wins over a word it starts with
  const alternatives = wanted.toSorted((a, b) => b.length - a.length).map(escape);
  return new RegExp(`(?<![\\p{L}\\p{N}_])(?:${alternatives.join("|")})(?![\\p{L}\\p{N}_])`, "giu");
};

/** Splits plain text around the watched words */
const marked = (text: string, at: number, pattern: RegExp | null): Rich[] => {
  if (!pattern) return [{ at, text, kind: "text" }];
  const parts: Rich[] = [];
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    if (match.index > last)
      parts.push({ at: at + last, text: text.slice(last, match.index), kind: "text" });
    parts.push({ at: at + match.index, text: match[0], kind: "mark" });
    last = match.index + match[0].length;
  }
  if (last < text.length) parts.push({ at: at + last, text: text.slice(last), kind: "text" });
  return parts;
};

/**
 * A message's text as runs to render as text: its http(s) links clickable, its `slsk://` links
 * opening what they point at, and the words in `watch` marked
 */
export const richText = (text: string, watch: string[] = []): Rich[] => {
  const pattern = watching(watch);
  const parts: Rich[] = [];
  let last = 0;
  for (const match of text.matchAll(LINK)) {
    const raw = match[0].replace(TRAILING, "");
    const at = match.index;
    const slsk = raw.toLowerCase().startsWith("slsk:") ? parseSlsk(raw) : null;
    const isUrl = !raw.toLowerCase().startsWith("slsk:");
    if (!slsk && !isUrl) continue;
    if (at > last) parts.push(...marked(text.slice(last, at), last, pattern));
    parts.push(
      slsk ? { at, text: raw, kind: "slsk", link: slsk } : { at, text: raw, kind: "url", url: raw }
    );
    last = at + raw.length;
  }
  if (last < text.length) parts.push(...marked(text.slice(last), last, pattern));
  return parts;
};

/** The action of a `/me` message, `/me waves` read as "waves"; null for any other message */
export const actionOf = (text: string): string | null =>
  /^\/me\s+\S/.test(text) ? text.replace(/^\/me\s+/, "") : null;
