import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { ownProfile, pictureOf } from "./profile.svelte";

describe("the picture a username shows", () => {
  afterEach(() => {
    app.state = null;
    vi.unstubAllGlobals();
  });

  it("is ours from our own profile, which the directory never holds, fetched once", async () => {
    const net = fakeNetwork((url) =>
      url.pathname === "/api/v0/profile" ? { pictureUrl: "/api/v0/profile/picture?v=1" } : undefined
    );
    app.state = { user: { username: "me" } } as never;
    expect(pictureOf("me", { info: { pictureUrl: "/peer/copy" } })).toBeUndefined();
    await vi.waitFor(() => expect(ownProfile.data).toBeDefined());
    expect(pictureOf("me")).toBe("/api/v0/profile/picture?v=1");
    expect(net.requests.filter((r) => r.url.pathname === "/api/v0/profile")).toHaveLength(1);
  });

  it("is anyone else's from what the caller holds", () => {
    app.state = { user: { username: "me" } } as never;
    expect(pictureOf("alice", { info: { pictureUrl: "/alice" } })).toBe("/alice");
    expect(pictureOf("alice", { info: null })).toBeUndefined();
  });
});
