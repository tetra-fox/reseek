import { afterEach, describe, expect, it, vi } from "vitest";

import { toasts } from "#lib/feedback/toast.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { unblockMany } from "./block.svelte";
import { users } from "./users.svelte";

describe("unblocking several users", () => {
  afterEach(() => {
    toasts.splice(0);
    users.clear();
    vi.unstubAllGlobals();
  });

  it("says so once, and its one Undo blocks each again as they were", async () => {
    const blocks: Record<string, unknown> = {
      alice: { reason: "spam", scopes: ["uploads", "messages"], expiresAt: null },
      bob: { reason: "", scopes: ["searches"], expiresAt: "2999-01-01T00:00:00Z" }
    };
    const net = fakeNetwork((url, method) => {
      const name = decodeURIComponent(url.pathname.split("/")[4]);
      if (url.pathname.endsWith("/summary")) return { username: name, block: blocks[name] ?? null };
      if (url.pathname.endsWith("/block") && method === "DELETE") {
        delete blocks[name];
        return { status: 204 };
      }
      if (url.pathname.endsWith("/block") && method === "PUT") return { username: name };
      return undefined;
    });
    await unblockMany(["alice", "bob"]);
    expect(toasts.map((t) => t.text)).toEqual(["2 users unblocked"]);
    toasts[0].action!.run();
    await vi.waitFor(() => expect(net.requests.filter((r) => r.method === "PUT")).toHaveLength(2));
    const again = net.requests
      .filter((r) => r.method === "PUT")
      .map((r) => [r.url.pathname.split("/")[4], r.body]);
    expect(again).toEqual([
      [
        "alice",
        { reason: "spam", scopes: ["uploads", "messages"], expiresAt: null, cancelTransfers: false }
      ],
      [
        "bob",
        {
          reason: "",
          scopes: ["searches"],
          expiresAt: "2999-01-01T00:00:00Z",
          cancelTransfers: false
        }
      ]
    ]);
  });
});
