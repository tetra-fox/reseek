/** A run of text starting at `at`: plain, or a web link to render as one */
export type TextPart = { at: number; text: string; url?: string };

/** http(s) links, up to the first space; trailing punctuation stays text */
const URL_PATTERN = /https?:\/\/[^\s<>"]+/g;
const TRAILING = /[.,;:!?)\]}'"]+$/;

/**
 * Splits text into plain runs and `http(s)://` links, for text from other users rendered as
 * text with only the links made clickable
 */
export const linkify = (text: string): TextPart[] => {
  const parts: TextPart[] = [];
  let last = 0;
  for (const match of text.matchAll(URL_PATTERN)) {
    const url = match[0].replace(TRAILING, "");
    const at = match.index;
    if (at > last) parts.push({ at: last, text: text.slice(last, at) });
    parts.push({ at, text: url, url });
    last = at + url.length;
  }
  if (last < text.length) parts.push({ at: last, text: text.slice(last) });
  return parts;
};
