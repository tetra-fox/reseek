import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { dismiss, toasts } from "#lib/feedback/toast.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { fileKey, Transfers } from "./transfers.svelte";

describe("the transfers store", () => {
  afterEach(() => {
    live.stop();
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

  it("keeps a transfer's end that a frame brought while a reload was reading", async () => {
    let answer: ((listing: unknown) => void) | undefined;
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/transfers/downloads")
        ? new Promise((resolve) => (answer = resolve))
        : undefined
    );
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
    const downloads = new Transfers("download");
    const running = transfer({ state: "InProgress", bytesTransferred: 530 });
    downloads.list.upsert(running);

    const loading = downloads.load();
    await vi.waitFor(() => expect(answer).toBeDefined());
    net.frame("transfers.updated", {
      ...running,
      state: "Completed, Succeeded",
      bytesTransferred: 1000
    });
    await vi.waitFor(() =>
      expect(downloads.list.get(running.id)?.state).toBe("Completed, Succeeded")
    );
    // the listing was read before the transfer ended, and answers after its frame
    answer!([{ username: running.username, directories: [{ files: [running] }] }]);
    await loading;

    expect(downloads.list.get(running.id)?.state).toBe("Completed, Succeeded");
    expect(downloads.list.get(running.id)?.bytesTransferred).toBe(1000);
    expect(downloads.loaded).toBe(true);
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
