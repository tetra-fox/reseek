/**
 * Where each header's lines end and what each line is in, from a list of lines in order, each at
 * a depth and some of them headers of the lines after them at a greater depth: `end[i]` is the
 * index past the last line under the header at `i`, `parent[i]` the index of the header line `i`
 * is under, -1 for none, `depth` and `header` each line's, and `deepest` the deepest header's depth
 */
export const nesting = <L>(
  lines: L[],
  depthOf: (line: L) => number,
  isHeader: (line: L) => boolean
) => {
  const end = new Int32Array(lines.length).fill(lines.length);
  const parent = new Int32Array(lines.length).fill(-1);
  const depth = new Int32Array(lines.length);
  const header = new Uint8Array(lines.length);
  const open: number[] = [];
  let deepest = 0;
  lines.forEach((line, i) => {
    depth[i] = depthOf(line);
    while (open.length && depth[open.at(-1)!] >= depth[i]) end[open.pop()!] = i;
    parent[i] = open.at(-1) ?? -1;
    if (!isHeader(line)) return;
    header[i] = 1;
    open.push(i);
    deepest = Math.max(deepest, depth[i]);
  });
  return { end, parent, depth, header, deepest };
};

/** A header drawn over the lines while the lines under it scroll */
export type Pin = {
  /** the header's index in the lines */
  index: number;
  /** how far below the lines' top it is drawn, in pixels; under 0 while it is pushed out */
  top: number;
  /** its place in the stack, 0 at the top, which it keeps until its lines' end pushes it up */
  slot: number;
};

/**
 * The headers pinned at the top of the lines, as nested section headers are: each header stays in
 * its slot, `slot(depth)` lines down, from when its own line passes the slot until its last line
 * does, which then pushes it up. `scrollTop` is how far the lines have scrolled under their top
 * and `row` a line's height, which every line shares.
 *
 * The result is in painting order, last on top: a lower slot under a higher one, and of headers
 * sharing a slot the deeper on top, so a pushed header goes under the one above it. A header
 * another covers exactly is left out
 */
export const pins = (
  { end, parent, depth, header, deepest }: ReturnType<typeof nesting>,
  scrollTop: number,
  row: number,
  slot: (depth: number) => number
): Pin[] => {
  const count = end.length;
  if (!count || row <= 0) return [];
  const first = Math.min(Math.max(Math.floor(scrollTop / row), 0), count - 1);
  // every header that can be pinned is over the line at the top or starts within the stack's height
  const candidates = new Set<number>();
  for (let i = parent[first]; i !== -1; i = parent[i]) candidates.add(i);
  for (let i = first; i < Math.min(count, first + slot(deepest) + 1); i++) {
    if (header[i]) candidates.add(i);
  }
  const found: (Pin & { depth: number })[] = [];
  for (const index of candidates) {
    const at = slot(depth[index]);
    if (index * row >= scrollTop + at * row || end[index] * row <= scrollTop) continue;
    found.push({
      index,
      top: Math.min(at * row, end[index] * row - scrollTop - row),
      slot: at,
      depth: depth[index]
    });
  }
  found.sort((a, b) => b.slot - a.slot || a.depth - b.depth);
  return found
    .filter((pin, k) => !found.slice(k + 1).some((above) => above.top === pin.top))
    .map(({ index, top, slot }) => ({ index, top, slot }));
};
