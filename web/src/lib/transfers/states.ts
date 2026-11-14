import type { Schemas } from "#lib/api/client.js";
import { SIGNED_MOST } from "#lib/files/attributes.js";
import { formatCount } from "#lib/format.js";
import { fold } from "#lib/text.js";

export type Transfer = Schemas["Transfer"];
export type TransferState = Schemas["TransferStates"];
export type Direction = "download" | "upload";

/**
 * Where a transfer stands: before it moves, moving, or how it ended. A state this page does not
 * know, from a daemon newer than it, is unknown: neither waiting, moving nor ended
 */
type Stage =
  | "requested"
  | "queued"
  | "starting"
  | "moving"
  | "succeeded"
  | "canceled"
  | "failed"
  | "unknown";

type Facts =
  | { stage: Exclude<Stage, "failed" | "unknown">; name: string }
  | { stage: "failed"; name: string; outcome: string };

/**
 * Each state's stage and its name alone, as it reads where the transfer's own figures don't
 * belong. A failed state also names the outcome the reports count it under
 */
const FACTS: Record<TransferState, Facts> = {
  "Requested": { stage: "requested", name: "Requested" },
  "Queued, Locally": { stage: "queued", name: "Queued" },
  "Queued, Remotely": { stage: "queued", name: "Queued" },
  "Initializing": { stage: "starting", name: "Initializing" },
  "InProgress": { stage: "moving", name: "In progress" },
  "Completed, Succeeded": { stage: "succeeded", name: "Succeeded" },
  "Completed, Cancelled": { stage: "canceled", name: "Canceled" },
  "Completed, TimedOut": { stage: "failed", name: "Timed out", outcome: "TimedOut" },
  "Completed, Errored": { stage: "failed", name: "Failed", outcome: "Errored" },
  "Completed, Rejected": { stage: "failed", name: "Rejected", outcome: "Rejected" },
  "Completed, Aborted": { stage: "failed", name: "Aborted", outcome: "Aborted" }
};

/**
 * The report outcomes of a failed transfer: a timeout, an error, a refusal or a size mismatch. A
 * cancel is not a failure
 */
export const FAILED_OUTCOMES = Object.values(FACTS).flatMap((f) =>
  f.stage === "failed" ? [f.outcome] : []
);

const stageOf = (state: TransferState): Stage =>
  Object.hasOwn(FACTS, state) ? FACTS[state].stage : "unknown";

/** It ended, so it can leave the list */
export const isEnded = (state: TransferState) => {
  const stage = stageOf(state);
  return stage === "succeeded" || stage === "canceled" || stage === "failed";
};

/** It ended other than successfully, so it can be tried again */
export const isRetryable = (state: TransferState) => {
  const stage = stageOf(state);
  return stage === "canceled" || stage === "failed";
};

/** It is waiting or moving, so it can be stopped */
export const isCancelable = (state: TransferState) => {
  const stage = stageOf(state);
  return stage === "requested" || stage === "queued" || stage === "starting" || stage === "moving";
};

export const isSucceeded = (state: TransferState) => stageOf(state) === "succeeded";

export const isFailed = (state: TransferState) => stageOf(state) === "failed";

export const isCanceled = (state: TransferState) => stageOf(state) === "canceled";

export const isQueued = (state: TransferState) => stageOf(state) === "queued";

/** It waits here, for a slot or a retry, and can be moved in the local queue */
export const isQueuedLocally = (state: TransferState) => state === "Queued, Locally";

/** It waits in the peer's queue, which can be asked for its place */
export const isQueuedRemotely = (state: TransferState) => state === "Queued, Remotely";

export const isRequested = (state: TransferState) => stageOf(state) === "requested";

/** It is starting or moving bytes */
export const isActive = (state: TransferState) => {
  const stage = stageOf(state);
  return stage === "starting" || stage === "moving";
};

/** It is moving bytes */
export const isMoving = (state: TransferState) => stageOf(state) === "moving";

/** A state's name alone: "Queued", "Failed", "Timed out"; an unknown state as the daemon says it */
export const stateName = (state: TransferState): string =>
  Object.hasOwn(FACTS, state) ? FACTS[state].name : state;

/** How an ended transfer's state reads in a sentence: "failed", "timed out", "canceled" */
export const endingText = (state: TransferState) => stateName(state).toLowerCase();

/**
 * What a file's row shows of its download behind it: waiting, moving with its bytes, done, or
 * failed; nothing for no download or a canceled one. A file just asked for is waiting
 */
export type Fill = "waiting" | "moving" | "done" | "failed" | null;

const FILLS: Record<Stage, Fill> = {
  requested: "waiting",
  queued: "waiting",
  starting: "waiting",
  moving: "moving",
  succeeded: "done",
  canceled: null,
  failed: "failed",
  unknown: null
};

export const fillOf = (t: Transfer | undefined, starting = false): Fill => {
  if (!t) return starting ? "waiting" : null;
  return FILLS[stageOf(t.state)];
};

/** The state chips above the table, each with the states it stands for */
export const STATE_CHIPS = [
  { id: "active", label: "Active", test: isActive },
  { id: "queued", label: "Queued", test: isQueued },
  { id: "requested", label: "Requested", test: isRequested },
  { id: "completed", label: "Succeeded", test: isSucceeded },
  { id: "errored", label: "Failed", test: isFailed },
  { id: "canceled", label: "Canceled", test: isCanceled }
] as const;

export type ChipId = (typeof STATE_CHIPS)[number]["id"];

/** The color a state is shown in */
export type Tone = "accent" | "success" | "info" | "neutral" | "warning" | "danger";

/**
 * A failure's word with its reason: "Failed: Connection refused". A reason that names the outcome
 * itself is not prefixed again: "transfer rejected: Too many megabytes" under Rejected gives
 * "Rejected: Too many megabytes", and "Timed out waiting for the peer" stands alone
 */
const withReason = (word: string, why: string | null) => {
  if (!why) return word;
  const cut = (text: string) => {
    const folded = fold(text, 40);
    return folded === null ? text : `${folded}...`;
  };
  if (why.toLowerCase().startsWith(word.toLowerCase())) return cut(why);
  const said = new RegExp(`^[^:]*\\b${word}: *`, "i");
  return `${word}: ${cut(why.replace(said, ""))}`;
};

/** How a transfer's state reads in its chip, its color, and the full text for its tooltip */
/** A queued download's place, null when the peer gave none: 0, or a -1 read unsigned */
export const placeOf = (t: Transfer): number | null =>
  t.placeInQueue && t.placeInQueue <= SIGNED_MOST ? t.placeInQueue : null;

export const stateChip = (t: Transfer): { text: string; tone: Tone; title: string | null } => {
  const tries = t.attempts > 1 ? ` ×${formatCount(t.attempts)}` : "";
  const why = t.exception ?? null;
  const s = t.state;
  const name = stateName(s);
  if (t.paused && isCancelable(s)) return { text: "Paused", tone: "neutral", title: null };
  switch (stageOf(s)) {
    case "moving":
      // whole percents rounded down, as a group's are, so a file never reads 100% before it is done
      return { text: `${Math.floor(t.percentComplete)}%`, tone: "accent", title: null };
    case "succeeded":
      return { text: name, tone: "success", title: null };
    case "starting":
      return { text: name + tries, tone: "info", title: null };
    case "queued": {
      const place = placeOf(t);
      const placed = place === null ? "" : ` (#${formatCount(place)})`;
      return {
        text: `${name}${placed}${tries}`,
        tone: t.attempts > 1 ? "warning" : "neutral",
        title: s
      };
    }
    case "requested":
      return { text: name + tries, tone: t.attempts > 1 ? "warning" : "neutral", title: null };
    case "failed": {
      const text = s === "Completed, Aborted" ? name : withReason(name, why);
      return { text: text + tries, tone: "danger", title: why };
    }
    case "canceled":
      return { text: name + tries, tone: "danger", title: why };
    case "unknown":
      return { text: name, tone: "neutral", title: null };
  }
};

/** What can be done to one transfer from its row, by its state and which way it goes */
export const rowActions = (t: Transfer, direction: Direction) => {
  const s = t.state;
  const download = direction === "download";
  const succeeded = isSucceeded(s);
  return {
    open: download && succeeded && !!t.localPath,
    downloadToDevice: download && succeeded && !!t.localPath,
    openFolder: download && succeeded && !!t.localPath,
    copyLocalPath: !!t.localPath,
    retry: download && isRetryable(s),
    cancel: isCancelable(s),
    remove: true,
    downloadAgain: download && succeeded,
    pause: download && isCancelable(s) && !t.paused,
    resume: download && !!t.paused && isCancelable(s),
    reorder: download && isQueuedLocally(s) && !t.paused,
    queuePosition: download && isQueuedRemotely(s),
    deny: !download && isCancelable(s),
    bump: !download && isQueued(s)
  };
};
