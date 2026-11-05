import { describe, expect, it } from "vitest";

import { STATES, transfer } from "#lib/testing/fixtures.js";

import { fillOf, rowActions, stateChip } from "./states";

describe("the row menu", () => {
  it("offers per state what the download can take", () => {
    const enabled = (state: string, localPath: string | null = null) =>
      Object.entries(rowActions(transfer({ state, localPath }), "download"))
        .filter(([, on]) => on)
        .map(([name]) => name);
    expect(enabled("Queued, Remotely")).toEqual(["cancel", "remove", "queuePosition"]);
    expect(enabled("Queued, Locally")).toEqual(["cancel", "remove"]);
    expect(enabled("InProgress")).toEqual(["cancel", "remove"]);
    expect(enabled("Completed, Succeeded", "/dl/a.flac")).toEqual([
      "open",
      "downloadToDevice",
      "openFolder",
      "copyLocalPath",
      "remove",
      "downloadAgain"
    ]);
    for (const state of STATES.filter(
      (s) => s.startsWith("Completed, ") && !s.endsWith("Succeeded")
    )) {
      expect(enabled(state)).toEqual(["retry", "remove"]);
    }
  });

  it("offers deny for uploads not done and bump for queued ones", () => {
    const upload = (state: string) =>
      rowActions(transfer({ state, direction: "Upload" }), "upload");
    expect(upload("Queued, Locally")).toMatchObject({
      bump: true,
      deny: true,
      retry: false,
      cancel: true
    });
    expect(upload("InProgress")).toMatchObject({ bump: false, deny: true });
    expect(upload("Completed, Succeeded")).toMatchObject({ bump: false, deny: false, open: false });
  });
});

describe("state chips", () => {
  it("read the state, the place in the queue, the attempts and the reason", () => {
    expect(stateChip(transfer({ placeInQueue: 12 })).text).toBe("Queued (#12)");
    expect(stateChip(transfer({ state: "Requested", attempts: 3 }))).toMatchObject({
      text: "Requested ×3",
      tone: "warning"
    });
    expect(
      stateChip(transfer({ state: "Completed, Errored", exception: "Connection refused" }))
    ).toMatchObject({
      text: "Failed: Connection refused",
      tone: "danger",
      title: "Connection refused"
    });
    expect(stateChip(transfer({ state: "Completed, TimedOut" })).text).toBe("Timed out");
    expect(stateChip(transfer({ state: "InProgress", percentComplete: 41.6 })).text).toBe("42%");
  });
});

describe("a file's download fill", () => {
  it("waits, moves, ends done or failed, and shows nothing once canceled", () => {
    const fill = (state: string) => fillOf(transfer({ state }));
    expect(fillOf(undefined)).toBe(null);
    expect(fillOf(undefined, true)).toBe("waiting");
    expect(
      ["Requested", "Queued, Remotely", "Initializing", "InProgress"].map((s) => fill(s))
    ).toEqual(["waiting", "waiting", "waiting", "moving"]);
    expect(
      ["Completed, Succeeded", "Completed, TimedOut", "Completed, Cancelled"].map((s) => fill(s))
    ).toEqual(["done", "failed", null]);
  });
});
