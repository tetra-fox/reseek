import { afterEach, describe, expect, it, vi } from "vitest";

import { live } from "#lib/live/live.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { flush } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";

import { unseen } from "./unseen.svelte";

describe("what finished unseen", () => {
  afterEach(() => {
    live.stop();
    vi.unstubAllGlobals();
  });

  it("counts transfers that finish or fail while their screen is closed, each once", async () => {
    const net = connected(() => undefined);
    const a = transfer({ state: "InProgress" });
    const b = transfer();
    const up = transfer({ direction: "Upload", state: "Completed, Succeeded" });
    net.frame("transfers.updated", a);
    net.frame("transfers.updated", { ...a, state: "Completed, Succeeded" });
    // a frame repeating the same outcome counts no more
    net.frame("transfers.updated", { ...a, state: "Completed, Succeeded" });
    net.frame("transfers.updated", { ...b, state: "Completed, Errored" });
    net.frame("transfers.updated", { ...b, state: "Completed, Cancelled" });
    net.frame("transfers.updated", up);
    await flush();
    expect(unseen.of("download")).toEqual({ succeeded: 1, failed: 1 });
    expect(unseen.of("upload")).toEqual({ succeeded: 1, failed: 0 });

    // the screen open clears its counts and takes what finishes meanwhile as seen
    const close = unseen.view("download");
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
    const c = transfer();
    net.frame("transfers.updated", { ...c, state: "Completed, Succeeded" });
    await flush();
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
    close();
    const d = transfer();
    net.frame("transfers.updated", { ...d, state: "Completed, TimedOut" });
    await flush();
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 1 });
  });
});
