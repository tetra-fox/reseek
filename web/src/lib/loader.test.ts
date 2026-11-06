import { describe, expect, it } from "vitest";

import { Loader } from "./loader.svelte";
import type { Outcome } from "./request";

/** A fetch whose answers the test hands out one at a time */
const slowFetch = () => {
  const pending: ((outcome: Outcome<number>) => void)[] = [];
  const fetch = () => new Promise<Outcome<number>>((resolve) => pending.push(resolve));
  const answer = (outcome: Outcome<number>) => pending.shift()!(outcome);
  return { fetch, pending, answer };
};

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("Loader", () => {
  it("takes each answer while loads keep being asked for faster than they come back", async () => {
    const { fetch, pending, answer } = slowFetch();
    const kept: number[] = [];
    const loader = new Loader(fetch, (data) => kept.push(data));

    const done = loader.load();
    void loader.load();
    void loader.load();
    expect(pending).toHaveLength(1);

    answer({ ok: true, data: 1 });
    await settle();
    expect(loader.data).toBe(1);
    expect(loader.loading).toBe(false);
    // the loads asked for while the first ran come to one more
    expect(pending).toHaveLength(1);

    void loader.load();
    answer({ ok: true, data: 2 });
    await settle();
    expect(loader.data).toBe(2);

    answer({ ok: true, data: 3 });
    await done;
    expect(loader.data).toBe(3);
    expect(kept).toEqual([1, 2, 3]);
    expect(pending).toHaveLength(0);
  });
});
