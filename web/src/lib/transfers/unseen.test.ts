import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { transfer } from "#lib/testing/fixtures.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { unseen } from "./unseen.svelte";

describe("what finished unseen", () => {
  afterEach(() => {
    live.stop();
    vi.unstubAllGlobals();
  });

  it("counts transfers that finish or fail while their screen is closed, each once", async () => {
    const net = fakeNetwork(() => undefined);
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
    const settle = () => new Promise((done) => setTimeout(done, 0));
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
    await settle();
    expect(unseen.of("download")).toEqual({ succeeded: 1, failed: 1 });
    expect(unseen.of("upload")).toEqual({ succeeded: 1, failed: 0 });

    // the screen open clears its counts and takes what finishes meanwhile as seen
    const close = unseen.view("download");
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
    const c = transfer();
    net.frame("transfers.updated", { ...c, state: "Completed, Succeeded" });
    await settle();
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 0 });
    close();
    const d = transfer();
    net.frame("transfers.updated", { ...d, state: "Completed, TimedOut" });
    await settle();
    expect(unseen.of("download")).toEqual({ succeeded: 0, failed: 1 });
  });
});
