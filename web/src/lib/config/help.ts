/** A run of an option's help text, and whether it was written between backticks */
export type HelpPart = { text: string; code: boolean };

/**
 * An option's help split at its backticks, so names and values written `like this` can show as
 * code. A backtick with no partner stays as text
 */
export const helpParts = (doc: string): HelpPart[] => {
  const parts: HelpPart[] = [];
  let rest = doc;
  for (;;) {
    const open = rest.indexOf("`");
    const close = open < 0 ? -1 : rest.indexOf("`", open + 1);
    if (close < 0) break;
    if (open > 0) parts.push({ text: rest.slice(0, open), code: false });
    if (close > open + 1) parts.push({ text: rest.slice(open + 1, close), code: true });
    rest = rest.slice(close + 1);
  }
  if (rest) parts.push({ text: rest, code: false });
  return parts;
};
