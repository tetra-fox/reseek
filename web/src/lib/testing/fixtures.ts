import { newId } from "#lib/ids.js";
import type { Transfer } from "#lib/transfers/states.js";

/** A queued download from alice, with `fields` over it */
export const transfer = (fields: Partial<Transfer> = {}): Transfer => ({
  id: newId(),
  username: "alice",
  direction: "Download",
  filename: "@@a\\Music\\Album\\01 One.flac",
  size: 1000,
  state: "Queued, Remotely",
  requestedAt: "2026-10-26T10:00:00Z",
  bytesTransferred: 0,
  bytesRemaining: 1000,
  averageSpeed: 0,
  percentComplete: 0,
  attempts: 1,
  removed: false,
  ...fields
});

/** The states a transfer can be seen in, the transient ones first */
export const STATES = [
  "Requested",
  "Queued, Remotely",
  "Queued, Locally",
  "Initializing",
  "InProgress",
  "Completed, Succeeded",
  "Completed, Cancelled",
  "Completed, TimedOut",
  "Completed, Errored",
  "Completed, Rejected",
  "Completed, Aborted"
];
