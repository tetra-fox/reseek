import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import { groupProgress } from "./progress";

describe("a group's progress", () => {
  it("counts finished files whole, leaves failed ones out and times the rest", () => {
    const progress = groupProgress([
      transfer({ state: "Completed, Succeeded", size: 1000, bytesTransferred: 1000 }),
      transfer({ state: "InProgress", size: 1000, bytesTransferred: 400, averageSpeed: 100 }),
      transfer({ state: "InProgress", size: 1000, bytesTransferred: 0, averageSpeed: 100 }),
      transfer({ state: "Completed, Errored", size: 5000, bytesTransferred: 10 })
    ]);
    expect(progress).toEqual({
      moved: 1400,
      size: 3000,
      percent: (1400 / 3000) * 100,
      speed: 200,
      eta: 8,
      running: true
    });
  });

  it("has no time left to tell while nothing moves", () => {
    const progress = groupProgress([transfer({ state: "Queued, Remotely" })]);
    expect([progress.eta, progress.running, progress.percent]).toEqual([null, false, 0]);
  });
});
