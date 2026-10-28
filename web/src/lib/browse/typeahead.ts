/** How long after the last key typed letters still add to what is being looked for */
export const TYPE_AHEAD_MS = 700;

/**
 * The index of the next name that starts with `typed`, ignoring case, looking from the one
 * after `from` and wrapping around; -1 when none does. A letter typed again and again steps
 * through the names starting with it
 */
export const typeAhead = (names: string[], from: number, typed: string): number => {
  const text = typed.toLowerCase();
  if (!text || !names.length) return -1;
  const repeated = [...text].every((c) => c === text[0]);
  const wanted = repeated ? text[0] : text;
  // a longer word goes on matching the name it found; a repeated letter moves to the next one
  const start = repeated || text.length === 1 ? from + 1 : Math.max(from, 0);
  for (let step = 0; step < names.length; step++) {
    const at = (start + step) % names.length;
    if (names[at].toLowerCase().startsWith(wanted)) return at;
  }
  return -1;
};
