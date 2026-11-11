import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import {
  FAILED_OUTCOMES,
  fillOf,
  isActive,
  isCancelable,
  isCanceled,
  isEnded,
  isFailed,
  isMoving,
  isQueued,
  isQueuedLocally,
  isQueuedRemotely,
  isRequested,
  isRetryable,
  isSucceeded,
  rowActions,
  stateChip,
  stateName,
  TRANSFER_STATES,
  type TransferState
} from "./states";

describe("each state", () => {
  const holding = (predicate: (state: TransferState) => boolean) =>
    TRANSFER_STATES.filter(predicate);

  it("is waiting, moving or ended", () => {
    expect(holding(isRequested)).toEqual(["Requested"]);
    expect(holding(isQueued)).toEqual(["Queued, Locally", "Queued, Remotely"]);
    expect(holding(isQueuedLocally)).toEqual(["Queued, Locally"]);
    expect(holding(isQueuedRemotely)).toEqual(["Queued, Remotely"]);
    expect(holding(isActive)).toEqual(["Initializing", "InProgress"]);
    expect(holding(isMoving)).toEqual(["InProgress"]);
    expect(holding(isCancelable)).toEqual([
      "Requested",
      "Queued, Locally",
      "Queued, Remotely",
      "Initializing",
      "InProgress"
    ]);
    expect(holding(isEnded)).toEqual([
      "Completed, Succeeded",
      "Completed, Cancelled",
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted"
    ]);
  });

  it("ended in success, a cancel or a failure", () => {
    expect(holding(isSucceeded)).toEqual(["Completed, Succeeded"]);
    expect(holding(isCanceled)).toEqual(["Completed, Cancelled"]);
    expect(holding(isFailed)).toEqual([
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted"
    ]);
    expect(holding(isRetryable)).toEqual([
      "Completed, Cancelled",
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted"
    ]);
    // the keys of the reports' per-outcome figures
    expect(FAILED_OUTCOMES).toEqual(["TimedOut", "Errored", "Rejected", "Aborted"]);
  });

  it("has a name, a chip and a fill", () => {
    const read = (state: TransferState) => {
      const t = transfer({ state, percentComplete: 40 });
      const chip = stateChip(t);
      return [stateName(state), chip.text, chip.tone, fillOf(t)];
    };
    expect(Object.fromEntries(TRANSFER_STATES.map((state) => [state, read(state)]))).toEqual({
      "Requested": ["Requested", "Requested", "neutral", "waiting"],
      "Queued, Locally": ["Queued", "Queued", "neutral", "waiting"],
      "Queued, Remotely": ["Queued", "Queued", "neutral", "waiting"],
      "Initializing": ["Initializing", "Initializing", "info", "waiting"],
      "InProgress": ["In progress", "40%", "accent", "moving"],
      "Completed, Succeeded": ["Succeeded", "Succeeded", "success", "done"],
      "Completed, Cancelled": ["Canceled", "Canceled", "danger", null],
      "Completed, TimedOut": ["Timed out", "Timed out", "danger", "failed"],
      "Completed, Errored": ["Failed", "Failed", "danger", "failed"],
      "Completed, Rejected": ["Rejected", "Rejected", "danger", "failed"],
      "Completed, Aborted": ["Aborted", "Aborted", "danger", "failed"]
    });
  });
});

describe("the row menu", () => {
  it("offers per state what the download can take", () => {
    const enabled = (state: TransferState, localPath: string | null = null) =>
      Object.entries(rowActions(transfer({ state, localPath }), "download"))
        .filter(([, on]) => on)
        .map(([name]) => name);
    expect(enabled("Queued, Remotely")).toEqual(["cancel", "remove", "pause", "queuePosition"]);
    expect(enabled("Queued, Locally")).toEqual(["cancel", "remove", "pause", "reorder"]);
    expect(enabled("InProgress")).toEqual(["cancel", "remove", "pause"]);
    // a paused download waits here: it resumes, and holds its place out of the queue's order
    expect(
      Object.entries(rowActions(transfer({ state: "Queued, Locally", paused: true }), "download"))
        .filter(([, on]) => on)
        .map(([name]) => name)
    ).toEqual(["cancel", "remove", "resume"]);
    expect(stateChip(transfer({ state: "Queued, Locally", paused: true })).text).toBe("Paused");
    expect(enabled("Completed, Succeeded", "/dl/a.flac")).toEqual([
      "open",
      "downloadToDevice",
      "openFolder",
      "copyLocalPath",
      "remove",
      "downloadAgain"
    ]);
    for (const state of [
      "Completed, Cancelled",
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted"
    ] as const) {
      expect(enabled(state)).toEqual(["retry", "remove"]);
    }
  });

  it("offers deny for uploads not done and bump for queued ones", () => {
    const upload = (state: TransferState) =>
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
    // rounded down, so a file a hair short of done never reads 100%
    expect(stateChip(transfer({ state: "InProgress", percentComplete: 99.6 })).text).toBe("99%");
  });

  it("say the outcome once when the reason names it too", () => {
    const failed = (state: TransferState, exception: string) =>
      stateChip(transfer({ state, exception })).text;
    expect(failed("Completed, Rejected", "transfer rejected: Too many megabytes")).toBe(
      "Rejected: Too many megabytes"
    );
    expect(failed("Completed, TimedOut", "Timed out waiting for the peer")).toBe(
      "Timed out waiting for the peer"
    );
    expect(failed("Completed, Errored", "I/O error: disk full")).toBe(
      "Failed: I/O error: disk full"
    );
    // an abort's reason is only in the tooltip
    expect(
      stateChip(transfer({ state: "Completed, Aborted", exception: "Size mismatch" }))
    ).toMatchObject({ text: "Aborted", title: "Size mismatch" });
  });
});

describe("a file's download fill", () => {
  it("is nothing without a download, and waiting for one just asked for", () => {
    expect(fillOf(undefined)).toBe(null);
    expect(fillOf(undefined, true)).toBe("waiting");
  });
});
