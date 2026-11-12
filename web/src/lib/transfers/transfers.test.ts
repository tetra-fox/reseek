import { afterEach, describe, expect, it, vi } from "vitest";

import { dismiss, toasts } from "#lib/feedback/toast.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { fileKey, Transfers } from "./transfers.svelte";
import { unseen } from "./unseen.svelte";

describe("the transfers store", () => {
  afterEach(() => {
    live.stop();
    vi.unstubAllGlobals();
    vi.useRealTimers();
    toasts.splice(0);
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
    const net = connected((url) =>
      url.pathname.endsWith("/transfers/downloads")
        ? new Promise((resolve) => (answer = resolve))
        : undefined
    );
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
  });

  it("reads again once frames were missed, and counts what ended meanwhile", async () => {
    const running = transfer({ state: "InProgress" });
    let listed = [running];
    const net = connected((url) =>
      url.pathname.endsWith("/transfers/downloads")
        ? [{ username: running.username, directories: [{ files: listed }] }]
        : url.pathname.endsWith("/queue")
          ? { ids: [] }
          : undefined
    );
    const downloads = new Transfers("download");
    // a store nothing read yet is read by the screen that needs it
    net.frame("lagged", {});
    await flush();
    expect(downloads.loaded).toBe(false);
    expect(downloads.list.size).toBe(0);
    await downloads.load();
    const before = unseen.of("download").succeeded;

    // the transfer finished while the stream missed its frames
    listed = [{ ...running, state: "Completed, Succeeded" }];
    net.frame("lagged", {});
    await vi.waitFor(() =>
      expect(downloads.list.get(running.id)?.state).toBe("Completed, Succeeded")
    );
    expect(unseen.of("download").succeeded).toBe(before + 1);

    // a stream opened anew without the frames missed
    const other = transfer();
    listed = [other];
    net.greet();
    await vi.waitFor(() => expect(downloads.list.get(other.id)).toBeDefined());
    expect(downloads.list.get(running.id)).toBeUndefined();
  });

  it("keeps the queue order a move gave over a read of it begun before", async () => {
    let answerOrder: ((order: unknown) => void) | undefined;
    const net = connected((url, method) => {
      if (url.pathname.endsWith("/queue") && method === "GET") {
        return new Promise((resolve) => (answerOrder = resolve));
      }
      if (url.pathname.endsWith("/queue")) return { ids: ["b", "a"] };
      if (url.pathname.endsWith("/transfers/downloads")) return [];
      return undefined;
    });
    const downloads = new Transfers("download");
    await downloads.load();
    await vi.waitFor(() => expect(answerOrder).toBeDefined());
    await downloads.move(["b"], "a");
    expect([...downloads.places]).toEqual([
      ["b", 1],
      ["a", 2]
    ]);
    // the read from before the move answers last
    answerOrder!({ ids: ["a", "b"] });
    await flush();
    expect(downloads.places.get("b")).toBe(1);

    // a frame says another tab moved them since
    net.frame("transfers.queue", { ids: ["a", "b"] });
    await vi.waitFor(() => expect(downloads.places.get("a")).toBe(1));
  });

  it("takes no move's answer over a frame that came while it was on its way", async () => {
    let answerMove: ((order: unknown) => void) | undefined;
    const net = connected((url, method) =>
      url.pathname.endsWith("/queue") && method === "POST"
        ? new Promise((resolve) => (answerMove = resolve))
        : undefined
    );
    const downloads = new Transfers("download");
    const moving = downloads.move(["b"], "a");
    await vi.waitFor(() => expect(answerMove).toBeDefined());
    net.frame("transfers.queue", { ids: ["c", "b", "a"] });
    await vi.waitFor(() => expect(downloads.places.get("c")).toBe(1));
    answerMove!({ ids: ["b", "a"] });
    await moving;
    expect(downloads.places.get("c")).toBe(1);
  });

  it("removes the transfers it named, with keepalive as the page is left", async () => {
    vi.useFakeTimers();
    const net = fakeNetwork(() => ({ ok: [], failed: [] }));
    const downloads = new Transfers("download");
    const done = [
      transfer({ state: "Completed, Succeeded" }),
      transfer({ state: "Completed, Errored" })
    ];
    for (const t of done) downloads.list.upsert(t);
    downloads.removeLater(done.map((t) => t.id));
    // one that finishes meanwhile is not among them
    downloads.list.upsert(transfer({ state: "Completed, Succeeded" }));
    window.dispatchEvent(new Event("pagehide"));
    await vi.advanceTimersByTimeAsync(0);
    expect(net.requests.map((r) => [r.method, r.url.pathname, r.body, r.keepalive])).toEqual([
      [
        "POST",
        "/api/v0/transfers/downloads/bulk",
        { action: "remove", ids: done.map((t) => t.id) },
        true
      ]
    ]);
  });
});
