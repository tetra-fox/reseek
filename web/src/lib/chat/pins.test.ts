import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";

import { pins } from "./pins.svelte";

const room = (name: string) => ({ kind: "room", name }) as const;
const user = (name: string) => ({ kind: "user", name }) as const;

describe("the pins", () => {
  afterEach(() => {
    pins.clear();
    vi.unstubAllGlobals();
  });

  it("puts a new pin last, and keeps the place of one already pinned", async () => {
    const network = fakeNetwork(() => ({ status: 204 }));
    pins.list = [room("jazz"), user("alice")];
    pins.set([room("indie"), room("jazz")], true);
    expect(pins.list).toEqual([room("jazz"), user("alice"), room("indie")]);
    await vi.waitFor(() => expect(network.requests).toHaveLength(1));
  });

  it("keeps a pin the list leaves out in its place through a move", () => {
    fakeNetwork(() => ({ status: 204 }));
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
    const answers: (() => void)[] = [];
    const network = fakeNetwork(
      () => new Promise((resolve) => answers.push(() => resolve({ status: 204 })))
    );
    pins.list = [room("a"), room("b"), room("c")];
    pins.move(room("a"), room("c"));
    pins.move(room("b"), null);
    pins.move(room("c"), null);
    expect(pins.list).toEqual([room("a"), room("b"), room("c")]);
    await vi.waitFor(() => expect(network.requests).toHaveLength(1));
    expect(network.requests[0].body).toEqual([room("b"), room("a"), room("c")]);

    answers[0]();
    await vi.waitFor(() => expect(network.requests).toHaveLength(2));
    expect(network.requests[1].body).toEqual([room("a"), room("b"), room("c")]);
    answers[1]();
    await vi.waitFor(() => expect(pins.has(room("a"))).toBe(true));
  });

  it("takes the daemon's list when a write fails", async () => {
    fakeNetwork((_url, method) =>
      method === "PUT" ? { status: 500, body: { error: "disk full" } } : [room("jazz")]
    );
    pins.list = [room("jazz"), room("indie")];
    pins.set([room("indie")], false);
    pins.set([room("indie")], true);
    await vi.waitFor(() => expect(pins.list).toEqual([room("jazz")]));
  });
});
