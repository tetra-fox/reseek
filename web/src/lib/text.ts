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

/** How many characters a text keeps whole at its end when it is cut in the middle */
const TAIL = 12;
/** A path's last part is kept whole up to this long; a longer one keeps its last `TAIL` */
const PATH_TAIL = 48;

/**
 * Where a text cut in the middle splits: the head, which gives way first, and the tail, which
 * stays. A path keeps its last part with the separator before it; other text, such as a file
 * name, keeps its last few characters, where names that share a start differ
 */
export const splitTail = (text: string, path = false): [string, string] => {
  if (path) {
    const cut = Math.max(text.lastIndexOf("\\"), text.lastIndexOf("/"));
    if (cut > 0 && text.length - cut <= PATH_TAIL) return [text.slice(0, cut), text.slice(cut)];
  }
  const cut = Math.max(0, text.length - TAIL);
  return [text.slice(0, cut), text.slice(cut)];
};

/** Whether a text holds letters of a right-to-left script, Hebrew, Arabic, Syriac, Thaana or N'Ko */
export const hasRtl = (text: string): boolean =>
  /[\u0590-\u08ff\ufb1d-\ufdff\ufe70-\ufeff]/.test(text);

/**
 * A path's parts and separators in order, each to be drawn as a run of its own, so a part in a
 * right-to-left script does not swap places with the parts around it
 */
export const pathParts = (text: string): string[] => text.split(/([\\/])/).filter(Boolean);
