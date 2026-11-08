import type { Schemas } from "#lib/api/client.js";
import { formatCount } from "#lib/format.js";

export type Transfer = Schemas["Transfer"];
export type Direction = "download" | "upload";

/**
 * The outcomes of a failed transfer: a timeout, an error, a refusal or a size mismatch. A
 * cancel is not a failure, nor is an end with no outcome
 */
export const FAILED_OUTCOMES = ["TimedOut", "Errored", "Rejected", "Aborted"];

/** The states of a failed transfer */
export const ERRORED = FAILED_OUTCOMES.map((outcome) => `Completed, ${outcome}`);

/** It ended other than successfully, so it can be tried again */
export const isRetryable = (state: string) =>
  state.includes("Completed") && state !== "Completed, Succeeded";

/** It is waiting or moving, so it can be stopped */
export const isCancelable = (state: string) =>
  [
    "InProgress",
    "Requested",
    "Queued",
    "Queued, Remotely",
    "Queued, Locally",
    "Initializing"
  ].includes(state);

/** It ended, so it can leave the list */
export const isRemovable = (state: string) => state.includes("Completed");

export const isQueued = (state: string) => state.includes("Queued");

export const isSucceeded = (state: string) => state === "Completed, Succeeded";

/** How a state reads where the transfer's own figures don't belong, as in its history */
const NAMES: Record<string, string> = {
  "InProgress": "In progress",
  "Initializing": "Initializing",
  "Requested": "Requested",
  "Completed, Succeeded": "Succeeded",
  "Completed, Errored": "Failed",
  "Completed, Rejected": "Rejected",
  "Completed, TimedOut": "Timed out",
  "Completed, Cancelled": "Canceled"
};

/** A state's name alone: "Queued", "Failed", "Timed out" */
export const stateName = (state: string) =>
  isQueued(state) ? "Queued" : (NAMES[state] ?? state.replace("Completed, ", ""));

/** How an ended transfer's state reads in a sentence: "failed", "timed out", "canceled" */
export const endingText = (state: string) => stateName(state).toLowerCase();

export const isActive = (state: string) => state === "InProgress" || state === "Initializing";

/**
 * What a file's row shows of its download behind it: waiting, moving with its bytes, done, or
 * failed; nothing for no download or a canceled one. A file just asked for is waiting
 */
export type Fill = "waiting" | "moving" | "done" | "failed" | null;

export const fillOf = (t: Transfer | undefined, starting = false): Fill => {
  if (!t) return starting ? "waiting" : null;
  if (t.state === "InProgress") return "moving";
  if (isSucceeded(t.state)) return "done";
  if (t.state === "Completed, Cancelled") return null;
  return isRetryable(t.state) ? "failed" : "waiting";
};

/** The state chips above the table, each with the states it stands for */
export const STATE_CHIPS = [
  { id: "active", label: "Active", test: isActive },
  { id: "queued", label: "Queued", test: (s: string) => isQueued(s) },
  { id: "requested", label: "Requested", test: (s: string) => s === "Requested" },
  { id: "completed", label: "Succeeded", test: isSucceeded },
  { id: "errored", label: "Failed", test: (s: string) => ERRORED.includes(s) },
  { id: "canceled", label: "Canceled", test: (s: string) => s === "Completed, Cancelled" }
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
  const cut = (text: string) => (text.length > 40 ? `${text.slice(0, 40)}...` : text);
  if (why.toLowerCase().startsWith(word.toLowerCase())) return cut(why);
  const said = new RegExp(`^[^:]*\\b${word}: *`, "i");
  return `${word}: ${cut(why.replace(said, ""))}`;
};

/** How a transfer's state reads in its chip, its color, and the full text for its tooltip */
export const stateChip = (t: Transfer): { text: string; tone: Tone; title: string | null } => {
  const tries = t.attempts > 1 ? ` ×${formatCount(t.attempts)}` : "";
  const why = t.exception ?? null;
  const s = t.state;
  const name = stateName(s);
  if (t.paused && !s.includes("Completed")) return { text: "Paused", tone: "neutral", title: null };
  // whole percents rounded down, as a group's are, so a file never reads 100% before it is done
  if (s === "InProgress")
    return { text: `${Math.floor(t.percentComplete)}%`, tone: "accent", title: null };
  if (s === "Completed, Succeeded") return { text: name, tone: "success", title: null };
  if (s === "Initializing") return { text: name + tries, tone: "info", title: null };
  if (isQueued(s)) {
    const place = t.placeInQueue ? ` (#${formatCount(t.placeInQueue)})` : "";
    return {
      text: `${name}${place}${tries}`,
      tone: t.attempts > 1 ? "warning" : "neutral",
      title: s
    };
  }
  if (s === "Requested") {
    return { text: name + tries, tone: t.attempts > 1 ? "warning" : "neutral", title: null };
  }
  if (["Completed, Errored", "Completed, Rejected", "Completed, TimedOut"].includes(s))
    return { text: withReason(name, why) + tries, tone: "danger", title: why };
  if (s.startsWith("Completed, ")) return { text: name + tries, tone: "danger", title: why };
  return { text: s, tone: "neutral", title: why };
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
    resume: download && !!t.paused && !isRemovable(s),
    reorder: download && s === "Queued, Locally" && !t.paused,
    queuePosition: download && s === "Queued, Remotely",
    deny: !download && !isRemovable(s),
    bump: !download && isQueued(s)
  };
};
