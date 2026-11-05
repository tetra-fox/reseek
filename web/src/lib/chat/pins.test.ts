import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { pins } from "./pins.svelte";

const room = (name: string) => ({ kind: "room", name }) as const;
const user = (name: string) => ({ kind: "user", name }) as const;

/** A daemon that takes every write at once, as its first */
const taking = () => fakeNetwork((_url, _method, body) => ({ revision: 1, pins: body }));

/**
 * A daemon whose writes commit one by one when `answer` says, each with the next revision, whose
 * reads answer `read()`, and whose stream sends what `tell` is given
 */
const daemon = (read: () => unknown = () => undefined) => {
  const answers: (() => void)[] = [];
  let revision = 0;
  const network = fakeNetwork((_url, method, body) =>
    method === "PUT"
      ? new Promise((resolve) => answers.push(() => resolve({ revision: ++revision, pins: body })))
      : read()
  );
  live.start();
  network.frame("hello", {
    principal: { name: "me", role: "ReadWrite" },
    resumed: false
  } as Schemas["Hello"]);
  const writes = () => network.requests.filter((r) => r.method === "PUT");
  return {
    writes,
    /** commits the `n`th write and lets the store take the answer */
    answer: async (n: number) => {
      await vi.waitFor(() => expect(writes()).toHaveLength(n));
      answers[n - 1]();
      await flush();
    },
    tell: async (pins: Schemas["ChatPins"]) => {
      network.frame("chat.pins", pins);
      await flush();
    },
    /** a stream opened again without the frames missed, as after a restart */
    reopen: () =>
      network.frame("hello", {
        principal: { name: "me", role: "ReadWrite" },
        resumed: false
      } as Schemas["Hello"])
  };
};

describe("the pins", () => {
  afterEach(() => {
    live.stop();
    pins.clear();
    vi.unstubAllGlobals();
  });

  it("puts a new pin last, and keeps the place of one already pinned", async () => {
    const network = taking();
    pins.list = [room("jazz"), user("alice")];
    pins.set([room("indie"), room("jazz")], true);
    expect(pins.list).toEqual([room("jazz"), user("alice"), room("indie")]);
    await vi.waitFor(() => expect(network.requests).toHaveLength(1));
  });

  it("keeps a pin the list leaves out in its place through a move", () => {
    taking();
    // alice's conversation is closed: the list shows jazz, indie, ambient
    pins.list = [room("jazz"), user("alice"), room("indie"), room("ambient")];
    pins.move(room("ambient"), room("indie"));
    expect(pins.list).toEqual([room("jazz"), user("alice"), room("ambient"), room("indie")]);
    pins.move(room("jazz"), null);
    expect(pins.list).toEqual([user("alice"), room("ambient"), room("indie"), room("jazz")]);
  });

  it("does not bring back a pin another tab took off while it was dragged", () => {
    pins.list = [room("jazz")];
    pins.move(room("indie"), null);
    expect(pins.list).toEqual([room("jazz")]);
  });

  it("sends one write at a time, then only the newest list", async () => {
    const { writes, answer } = daemon();
    pins.list = [room("a"), room("b"), room("c")];
    pins.move(room("a"), room("c"));
    pins.move(room("b"), null);
    pins.move(room("c"), null);
    expect(pins.list).toEqual([room("a"), room("b"), room("c")]);
    await vi.waitFor(() => expect(writes()).toHaveLength(1));
    expect(writes()[0].body).toEqual([room("b"), room("a"), room("c")]);

    await answer(1);
    await vi.waitFor(() => expect(writes()).toHaveLength(2));
    expect(writes()[1].body).toEqual([room("a"), room("b"), room("c")]);
    await answer(2);
    expect(pins.list).toEqual([room("a"), room("b"), room("c")]);
  });

  it("takes the daemon's list when a write fails", async () => {
    fakeNetwork((_url, method) =>
      method === "PUT"
        ? { status: 500, body: { error: "disk full" } }
        : { revision: 0, pins: [room("jazz")] }
    );
    pins.list = [room("jazz"), room("indie")];
    pins.set([room("indie")], false);
    pins.set([room("indie")], true);
    await vi.waitFor(() => expect(pins.list).toEqual([room("jazz")]));
  });

  it("keeps the newest list when the stream tells of an older one late", async () => {
    const { answer, tell } = daemon();
    pins.set([room("jazz")], true);
    await answer(1);
    pins.set([user("alice")], true);
    await answer(2);
    // the change frame of the first write, behind the answer to the second
    await tell({ revision: 1, pins: [room("jazz")] });
    expect(pins.list).toEqual([room("jazz"), user("alice")]);
  });

  it("keeps the newest list when a read answered before the last write comes late", async () => {
    let answerRead = () => {};
    const { answer } = daemon(
      () =>
        new Promise(
          (resolve) => (answerRead = () => resolve({ revision: 1, pins: [room("jazz")] }))
        )
    );
    const reading = pins.read();
    pins.set([room("jazz")], true);
    await answer(1);
    pins.set([user("alice")], true);
    await answer(2);
    answerRead();
    await reading;
    expect(pins.list).toEqual([room("jazz"), user("alice")]);
  });

  it("takes another tab's list told during a write once the write ends before it", async () => {
    const { answer, tell } = daemon();
    pins.set([room("jazz")], true);
    // another tab's write committed after this one's, its frame here before this one's answer
    await tell({ revision: 2, pins: [room("indie")] });
    expect(pins.list).toEqual([room("jazz")]);
    await answer(1);
    expect(pins.list).toEqual([room("indie")]);
  });

  it("takes the daemon's list after a restart on an older database", async () => {
    const { tell, reopen } = daemon();
    await tell({ revision: 50, pins: [room("jazz"), room("indie")] });
    // the daemon's database was put back from a backup, which counts from where it was
    reopen();
    await tell({ revision: 30, pins: [room("jazz")] });
    expect(pins.list).toEqual([room("jazz")]);
  });
});
