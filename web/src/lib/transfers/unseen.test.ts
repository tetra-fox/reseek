import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { unseen } from "./unseen.svelte";

const metrics = (succeeded: number, failed: number) => {
  const direction = {
    inProgress: { files: 0, users: 0, averageSpeed: 0, totalSpeed: 0 },
    queued: { files: 0, users: 0, bytes: 0 },
    completed: { succeeded, failed, bytes: 0 }
  };
  return { downloads: direction, uploads: direction } as unknown as Schemas["TransferMetrics"];
};

describe("what finished unseen", () => {
  afterEach(() => {
    live.stop();
    vi.unstubAllGlobals();
  });

  it("counts from the totals at load and from each visit on", async () => {
    const net = fakeNetwork(() => undefined);
    live.start();
    const settle = () => new Promise((done) => setTimeout(done, 0));
    net.frame("metrics.update", metrics(5, 1));
    await settle();
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
    net.frame("metrics.update", metrics(8, 2));
    await settle();
    expect(unseen.of("download")).toEqual({ succeeded: 3, failed: 1 });
    unseen.seen("download");
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
    // a restarted daemon starts its totals over
    net.frame("metrics.update", metrics(1, 0));
    await settle();
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
  });
});
