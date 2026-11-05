import { afterEach, describe, expect, it, vi } from "vitest";

import { dismiss, toasts } from "#lib/feedback/toast.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { fileKey, Transfers } from "./transfers.svelte";

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

  it("leaves a removal that can still be undone out of the lookup by file", () => {
    vi.useFakeTimers();
    const downloads = new Transfers("download");
    const t = transfer();
    downloads.list.upsert(t);
    const key = fileKey(t.username, t.filename);
    downloads.removeLater([t.id]);
    expect(downloads.byFile.has(key)).toBe(false);
    toasts[0].action!.run();
    dismiss(toasts[0].id);
    expect(downloads.byFile.get(key)?.id).toBe(t.id);
    vi.useRealTimers();
  });
});
