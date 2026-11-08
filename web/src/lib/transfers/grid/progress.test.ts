import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import { groupProgress } from "./progress";

describe("a group's progress", () => {
  it("counts finished files whole, leaves failed ones out and times the slowest moving file", () => {
    const progress = groupProgress([
      transfer({ state: "Completed, Succeeded", size: 1000, bytesTransferred: 1000 }),
      transfer({ state: "InProgress", size: 1000, bytesTransferred: 400, remainingTime: 6_000 }),
      transfer({ state: "InProgress", size: 1000, bytesTransferred: 0, remainingTime: 7_500 }),
      transfer({ state: "Completed, Errored", size: 5000, bytesTransferred: 10 })
    ]);
    expect(progress).toEqual({
      moved: 1400,
      size: 3000,
      percent: (1400 / 3000) * 100,
      eta: 8,
      running: true
    });
  });

  it("has no time left to tell while a file waits or stalls", () => {
    const moving = transfer({ state: "InProgress", size: 1000, remainingTime: 2_000 });
    const queued = groupProgress([moving, transfer({ state: "Queued, Remotely" })]);
    expect([queued.eta, queued.running]).toEqual([null, true]);
    const stalled = transfer({ state: "InProgress", size: 1000, remainingTime: null });
    expect(groupProgress([moving, stalled]).eta).toBe(null);
    const idle = groupProgress([transfer({ state: "Queued, Remotely" })]);
    expect([idle.eta, idle.running, idle.percent]).toEqual([null, false, 0]);
  });
});
