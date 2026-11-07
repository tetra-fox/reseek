import type { FieldView } from "./fields";
import type { Field } from "./sections";

/**
 * Whether an option answers what was typed in the settings search: every word typed is found,
 * ignoring case, in its label, its key, its help, its environment variable or its flag.
 * Underscores and dots count as spaces, so `share downloads` finds `shares.share_downloads`
 */
export const fieldMatches = (field: Field, info: FieldView | undefined, query: string) => {
  const words = query.toLowerCase().replace(/[_.]/g, " ").split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const text = [field.label, field.key, info?.doc, info?.envVar, info?.cliFlag]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  const loose = text.replace(/[_.-]/g, " ");
  return words.every((w) => text.includes(w) || loose.includes(w));
};

/**
 * The section the reader is in: the last one whose top has reached `line`, the first while none
 * has; the last once the page is scrolled to its end, so a short section at the bottom can still
 * be the current one
 */
export const currentSection = (
  tops: { id: string; top: number }[],
  line: number,
  atEnd = false
): string | null => {
  if (!tops.length) return null;
  if (atEnd) return tops.at(-1)!.id;
  let current = tops[0].id;
  for (const { id, top } of tops) {
    if (top <= line) current = id;
    else break;
  }
  return current;
};
