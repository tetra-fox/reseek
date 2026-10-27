import type { Schemas } from "#lib/api/client.js";

import {
  ERRORED,
  isCancellable,
  isRemovable,
  isRetryable,
  isSucceeded,
  type Transfer
} from "./states";

export type BulkAction = Schemas["BulkAction"];

/** The scopes of the three bulk buttons, the first of each the default */
export const SCOPES = {
  retry: [
    { id: "errored", label: "Retry All Errored", test: (s: string) => ERRORED.includes(s) },
    {
      id: "cancelled",
      label: "Retry All Cancelled",
      test: (s: string) => s === "Completed, Cancelled"
    },
    { id: "all", label: "Retry All", test: isRetryable }
  ],
  cancel: [
    { id: "all", label: "Cancel All", test: isCancellable },
    {
      id: "queued",
      label: "Cancel All Queued",
      test: (s: string) => s === "Queued, Locally" || s === "Queued, Remotely" || s === "Queued"
    },
    { id: "inProgress", label: "Cancel All In Progress", test: (s: string) => s === "InProgress" }
  ],
  remove: [
    { id: "succeeded", label: "Remove All Succeeded", test: isSucceeded },
    { id: "errored", label: "Remove All Errored", test: (s: string) => ERRORED.includes(s) },
    {
      id: "cancelled",
      label: "Remove All Cancelled",
      test: (s: string) => s === "Completed, Cancelled"
    },
    { id: "completed", label: "Remove All Completed", test: isRemovable }
  ]
} as const;

/** Whether `action` applies to a transfer in `state`, so it is worth sending */
export const applies = (action: BulkAction, state: string) =>
  action === "retry" ? isRetryable(state) : action === "cancel" ? isCancellable(state) : true;

/** The bulk call for `action` on the transfers it applies to, null when there are none */
export const bulkRequest = (
  action: BulkAction,
  transfers: Transfer[]
): Schemas["BulkRequest"] | null => {
  const ids = transfers.filter((t) => applies(action, t.state)).map((t) => t.id);
  return ids.length ? { action, ids } : null;
};

/** How many active transfers a cancel would stop, which it asks about first */
export const activeAmong = (transfers: Transfer[]) =>
  transfers.filter((t) => t.state === "InProgress").length;

const PAST = { cancel: "cancelled", remove: "removed", retry: "retried" } as const;

/** One line for what a bulk call did: "12 removed, 1 failed: <why>" */
export const summary = (action: BulkAction, result: Schemas["BulkResult"]): string => {
  const done = `${result.ok.length} ${PAST[action]}`;
  if (!result.failed.length) return done;
  return `${done}, ${result.failed.length} failed: ${result.failed[0].error}`;
};
