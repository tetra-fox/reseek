import { describe, expect, it } from "vitest";

import { pullOlder } from "./older";

/** a history `pages` pages long, read from the newest */
const history = (pages: number) => {
  let read = 1;
  const settled: boolean[] = [];
  return {
    read: () => read,
    settled,
    older: async () => void (read = Math.min(pages, read + 1)),
    more: () => read < pages,
    settle: async (held: boolean) => void settled.push(held)
  };
};

describe("reading older history", () => {
  it("reads one page for a pull, the view kept on its line", async () => {
    const h = history(10);
    await pullOlder({ ...h, held: () => false });
    expect(h.read()).toBe(2);
    expect(h.settled).toEqual([false]);
  });

  it("stops once the scrollbar is let go", async () => {
    const h = history(10);
    let holding = true;
    await pullOlder({
      ...h,
      held: () => holding,
      settle: async (held) => {
        h.settled.push(held);
        if (h.read() === 4) holding = false;
      }
    });
    expect(h.read()).toBe(5);
    expect(h.settled).toEqual([true, true, true, false]);
  });
});
