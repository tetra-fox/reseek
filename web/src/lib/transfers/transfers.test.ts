import { afterEach, describe, expect, it, vi } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";
import { toasts } from "#lib/toast.svelte.js";

import { Transfers } from "./transfers.svelte";

describe("the transfers store", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    toasts.splice(0);
  });

  it("leaves a failed first load to the page", async () => {
    fakeNetwork(() => ({ status: 500, body: { error: "database is locked" } }));
    const uploads = new Transfers("upload");
    await uploads.load();
    expect(uploads.error).toBe("database is locked");
    expect(toasts).toHaveLength(0);
  });

  it("keeps why the place in the queue is unknown until an ask succeeds", async () => {
    let up = false;
    fakeNetwork(() => (up ? 4 : { status: 500, body: { error: "the peer is offline" } }));
    const downloads = new Transfers("download");
    const t = transfer();
    downloads.list.upsert(t);
    await downloads.placeInQueue(t);
    expect(downloads.queueErrors.get(t.id)).toBe("the peer is offline");
    up = true;
    await downloads.placeInQueue(t);
    expect(downloads.queueErrors.has(t.id)).toBe(false);
    expect(downloads.list.get(t.id)?.placeInQueue).toBe(4);
  });
});
