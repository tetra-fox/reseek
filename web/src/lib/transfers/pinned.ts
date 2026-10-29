import type { Item } from "./view";

/** How far a line is nested: a group header at its group's depth, a row one past its group's */
export const depthOf = (item: Item) =>
  item.type === "group" ? item.group.depth : item.group ? item.group.depth + 1 : 0;

/**
 * Where each group ends and what each line is in, from the lines in order: `end[i]` is the index
 * past the last line of the group whose header is at `i`, `parent[i]` the index of the header of
 * the group line `i` is in, -1 for none, and `deepest` the deepest header's depth
 */
export const nesting = (items: Item[]) => {
  const end = new Int32Array(items.length).fill(items.length);
  const parent = new Int32Array(items.length).fill(-1);
  const open: number[] = [];
  let deepest = 0;
  items.forEach((item, i) => {
    const depth = depthOf(item);
    while (open.length && depthOf(items[open.at(-1)!]) >= depth) end[open.pop()!] = i;
    parent[i] = open.at(-1) ?? -1;
    if (item.type !== "group") return;
    open.push(i);
    deepest = Math.max(deepest, depth);
  });
  return { end, parent, deepest };
};

/** A group header drawn over the rows while its group scrolls */
export type Pin = {
  /** the header's index in the lines */
  index: number;
  /** how far below the rows' top it is drawn, in pixels; under 0 while it is pushed out */
  top: number;
  /** its place in the stack, 0 at the top, which it keeps until its group's end pushes it up */
  slot: number;
};

/**
 * The group headers pinned at the top of the rows, as nested section headers are: each group's
 * header stays in its slot, `slot(depth)` lines down, from when its own line passes the slot until
 * the group's last line does, which then pushes it up. `scrollTop` is how far the rows have
 * scrolled under their top and `row` a line's height, which every line shares.
 *
 * The result is in painting order, last on top: a lower slot under a higher one, and of headers
 * sharing a slot the deeper on top, so a pushed header goes under the one above it. A header
 * another covers exactly is left out
 */
export const pins = (
  items: Item[],
  { end, parent, deepest }: ReturnType<typeof nesting>,
  scrollTop: number,
  row: number,
  slot: (depth: number) => number
): Pin[] => {
  if (!items.length || row <= 0) return [];
  const first = Math.min(Math.max(Math.floor(scrollTop / row), 0), items.length - 1);
  // every header that can be pinned is in the line at the top or starts within the stack's height
  const candidates = new Set<number>();
  for (let i = parent[first]; i !== -1; i = parent[i]) candidates.add(i);
  for (let i = first; i < Math.min(items.length, first + slot(deepest) + 1); i++) {
    if (items[i].type === "group") candidates.add(i);
  }
  const found: (Pin & { depth: number })[] = [];
  for (const index of candidates) {
    const depth = depthOf(items[index]);
    const at = slot(depth);
    if (index * row >= scrollTop + at * row || end[index] * row <= scrollTop) continue;
    found.push({
      index,
      top: Math.min(at * row, end[index] * row - scrollTop - row),
      slot: at,
      depth
    });
  }
  found.sort((a, b) => b.slot - a.slot || a.depth - b.depth);
  return found
    .filter((pin, k) => !found.slice(k + 1).some((above) => above.top === pin.top))
    .map(({ index, top, slot }) => ({ index, top, slot }));
};
