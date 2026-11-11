import type { Schemas } from "#lib/api/client.js";
import { formatCount } from "#lib/format.js";

import {
  isCancelable,
  isCanceled,
  isEnded,
  isFailed,
  isMoving,
  isQueued,
  isRetryable,
  type Transfer,
  type TransferState
} from "./states";

export type BulkAction = Schemas["BulkAction"];

/** The scopes of the three bulk buttons, the first of each the default */
export const SCOPES = {
  retry: [
    { id: "errored", label: "Retry all failed", test: isFailed },
    { id: "canceled", label: "Retry all canceled", test: isCanceled },
    { id: "all", label: "Retry all", test: isRetryable }
  ],
  cancel: [
    { id: "all", label: "Cancel all", test: isCancelable },
    { id: "queued", label: "Cancel all queued", test: isQueued },
    { id: "inProgress", label: "Cancel all in progress", test: isMoving }
  ],
  remove: [
    { id: "errored", label: "Remove all failed", test: isFailed },
    { id: "canceled", label: "Remove all canceled", test: isCanceled },
    { id: "completed", label: "Remove all finished", test: isEnded }
  ]
} as const;

/** Whether `action` applies to a transfer in `state`, so it is worth sending */
export const applies = (action: BulkAction, state: TransferState) =>
  action === "retry"
    ? isRetryable(state)
    : action === "cancel" || action === "pause" || action === "resume"
      ? isCancelable(state)
      : true;

/** The bulk call for `action` on the transfers it applies to, null when there are none */
export const bulkRequest = (
  action: BulkAction,
  transfers: Transfer[]
): Schemas["BulkRequest"] | null => {
  const ids = transfers.filter((t) => applies(action, t.state)).map((t) => t.id);
  return ids.length ? { action, ids } : null;
};

/** What pausing downloads costs, said before it is done */
export const PAUSE_WARNING = "Partial files are kept. You lose your place in each user's queue.";

/** How many active transfers a cancel would stop, which it asks about first */
export const activeAmong = (transfers: Transfer[]) =>
  transfers.filter((t) => isMoving(t.state)).length;

const PAST = {
  cancel: "canceled",
  remove: "removed",
  retry: "retried",
  pause: "paused",
  resume: "resumed"
} as const;

/** One line for what a bulk call did: "12 removed, 1 failed: <why>" */
export const summary = (action: BulkAction, result: Schemas["BulkResult"]): string => {
  const done = `${formatCount(result.ok.length)} ${PAST[action]}`;
  if (!result.failed.length) return done;
  return `${done}, ${formatCount(result.failed.length)} failed: ${result.failed[0].error}`;
};
