import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { notifier } from "#lib/chat/notify.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import "./notices.svelte";

describe("transfer notifications", () => {
  afterEach(() => {
    live.stop();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("tell of a batch only when it has several files, once all are done", async () => {
    const told = vi.spyOn(notifier, "other").mockImplementation(() => {});
    const batches: Record<string, { fileCount: number; completedCount: number }> = {
      one: { fileCount: 1, completedCount: 1 },
      album: { fileCount: 2, completedCount: 2 }
    };
    const net = fakeNetwork((url) => {
      const id = url.pathname.split("/").pop()!;
      const batch = batches[id];
      return batch ? { ...batch, failedCount: 0 } : undefined;
    });
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
    const settle = async () => {
      for (let i = 0; i < 5; i++) await new Promise((done) => setTimeout(done, 0));
    };
    const done = { state: "Completed, Succeeded" };
    net.frame("transfers.updated", transfer({ ...done, batchId: "one" }));
    await settle();
    expect(told.mock.calls.map((c) => c[0])).toEqual(["Download finished"]);
    net.frame("transfers.updated", transfer({ ...done, batchId: "album" }));
    net.frame("transfers.updated", transfer({ ...done, batchId: "album" }));
    await settle();
    expect(told.mock.calls.map((c) => c[0])).toEqual([
      "Download finished",
      "Download finished",
      "Download finished",
      "Folder finished: Album"
    ]);
  });
});
