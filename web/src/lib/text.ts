/** A run of text starting at `at`: plain, or a web link to render as one */
export type TextPart = { at: number; text: string; url?: string };

/** Links with a scheme `schemes` matches, such as `https?`, each up to the first space */
export const linkPattern = (schemes: string) => new RegExp(`(?:${schemes}):\\/\\/[^\\s<>"]+`, "gi");
const WEB = linkPattern("https?");
const TRAILING = /[.,;:!?)\]}'"]+$/;

/** The links `pattern` finds in `text`, where each starts, trailing punctuation left out */
export const links = (text: string, pattern: RegExp): { at: number; url: string }[] =>
  Array.from(text.matchAll(pattern), (match) => ({
    at: match.index,
    url: match[0].replace(TRAILING, "")
  }));

/**
 * Splits text into plain runs and `http(s)://` links, for text from other users rendered as
 * text with only the links made clickable
 */
export const linkify = (text: string): TextPart[] => {
  const parts: TextPart[] = [];
  let last = 0;
  for (const { at, url } of links(text, WEB)) {
    if (at > last) parts.push({ at: last, text: text.slice(last, at) });
    parts.push({ at, text: url, url });
    last = at + url.length;
  }
  if (last < text.length) parts.push({ at: last, text: text.slice(last) });
  return parts;
};

/** A run of a search snippet, `hit` where the search matched */
export type SnippetPart = { at: number; text: string; hit: boolean };

const ENTITIES: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#39;": "'",
  "&#x27;": "'"
};

/**
 * A search snippet, HTML-escaped with the matches inside `<mark>`, as runs of text to render as
 * text, never as HTML
 */
export const snippetParts = (snippet: string): SnippetPart[] => {
  const parts: SnippetPart[] = [];
  let at = 0;
  let hit = false;
  for (const piece of snippet.split(/(<\/?mark>)/)) {
    if (piece === "<mark>") hit = true;
    else if (piece === "</mark>") hit = false;
    else if (piece) {
      parts.push({
        at,
        text: piece.replace(/&(?:amp|lt|gt|quot|#39|#x27);/g, (e) => ENTITIES[e]),
        hit
      });
      at += piece.length;
    }
  }
  return parts;
};
