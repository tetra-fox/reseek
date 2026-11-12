import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { userSummary } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";

import { users } from "./users.svelte";

describe("user summaries", () => {
  afterEach(() => {
    live.stop();
    users.clear();
    vi.unstubAllGlobals();
  });

  /** A daemon whose summary reads each wait until the test answers them, in any order */
  const held = () => {
    const reads: ((blocked: boolean) => void)[] = [];
    const net = connected((url) =>
      url.pathname.endsWith("/summary")
        ? new Promise((answer) =>
            reads.push((blocked) =>
              answer(userSummary("alice", blocked ? { block: { username: "alice" } as never } : {}))
            )
          )
        : undefined
    );
    return { net, reads };
  };
  const blocked = () => !!users.get("alice")?.block;

  it("read fresh after a block show it, though a read from before answers last", async () => {
    const { reads } = held();
    const before = users.load("alice");
    await vi.waitFor(() => expect(reads).toHaveLength(1));
    // the block is made, then read fresh: that read does not join the one on its way
    const fresh = users.load("alice", { fresh: true });
    await vi.waitFor(() => expect(reads).toHaveLength(2));
    reads[1](true);
    await fresh;
    expect(blocked()).toBe(true);
    reads[0](false);
    await before;
    expect(blocked()).toBe(true);
  });

  it("keep what a frame told over a read on its way before it", async () => {
    const { net, reads } = held();
    const loading = users.load("alice");
    await vi.waitFor(() => expect(reads).toHaveLength(1));
    net.frame("users.updated", userSummary("alice", { block: { username: "alice" } as never }));
    await flush();
    expect(blocked()).toBe(true);
    reads[0](false);
    await loading;
    expect(blocked()).toBe(true);
  });
});
