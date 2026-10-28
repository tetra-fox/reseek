import type { Schemas } from "#lib/api/client.js";

export type Transfer = Schemas["Transfer"];
export type Direction = "download" | "upload";

/** The failures a transfer can end in other than a cancel */
export const ERRORED = [
  "Completed, TimedOut",
  "Completed, Errored",
  "Completed, Rejected",
  "Completed, Aborted"
];

/** It ended other than successfully, so it can be tried again */
export const isRetryable = (state: string) =>
  state.includes("Completed") && state !== "Completed, Succeeded";

/** It is waiting or moving, so it can be stopped */
export const isCancellable = (state: string) =>
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

const ENDINGS: Record<string, string> = { Errored: "failed", TimedOut: "timed out" };

/** How an ended transfer's state reads in a sentence: "failed", "timed out", "cancelled" */
export const endingText = (state: string) => {
  const word = state.replace("Completed, ", "");
  return ENDINGS[word] ?? word.toLowerCase();
};

export const isActive = (state: string) => state === "InProgress" || state === "Initializing";

/** The state chips above the table, each with the states it stands for */
export const STATE_CHIPS = [
  { id: "active", label: "Active", test: isActive },
  { id: "queued", label: "Queued", test: (s: string) => isQueued(s) },
  { id: "requested", label: "Requested", test: (s: string) => s === "Requested" },
  { id: "completed", label: "Succeeded", test: isSucceeded },
  { id: "errored", label: "Failed", test: (s: string) => ERRORED.includes(s) },
  { id: "cancelled", label: "Cancelled", test: (s: string) => s === "Completed, Cancelled" }
] as const;

export type ChipId = (typeof STATE_CHIPS)[number]["id"];

/** The colour a state is shown in */
export type Tone = "accent" | "success" | "info" | "neutral" | "warning" | "danger";

/** How a transfer's state reads in its chip, its colour, and the full text for its tooltip */
export const stateChip = (t: Transfer): { text: string; tone: Tone; title: string | null } => {
  const tries = t.attempts > 1 ? ` ×${t.attempts}` : "";
  const why = t.exception ?? null;
  const short = why && why.length > 40 ? `${why.slice(0, 40)}...` : why;
  const reason = (word: string) => (short ? `${word}: ${short}` : word);
  const s = t.state;
  if (s === "InProgress")
    return { text: `${Math.round(t.percentComplete)}%`, tone: "accent", title: null };
  if (s === "Completed, Succeeded") return { text: "Succeeded", tone: "success", title: null };
  if (s === "Initializing") return { text: `Initializing${tries}`, tone: "info", title: null };
  if (isQueued(s)) {
    const place = t.placeInQueue ? ` (#${t.placeInQueue})` : "";
    return {
      text: `Queued${place}${tries}`,
      tone: t.attempts > 1 ? "warning" : "neutral",
      title: s
    };
  }
  if (s === "Requested") {
    return { text: `Requested${tries}`, tone: t.attempts > 1 ? "warning" : "neutral", title: null };
  }
  if (s === "Completed, Errored")
    return { text: reason("Failed") + tries, tone: "danger", title: why };
  if (s === "Completed, Rejected")
    return { text: reason("Rejected") + tries, tone: "danger", title: why };
  if (s === "Completed, TimedOut")
    return { text: reason("Timed out") + tries, tone: "danger", title: why };
  if (s.startsWith("Completed, ")) {
    return { text: s.slice("Completed, ".length) + tries, tone: "danger", title: why };
  }
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
    cancel: isCancellable(s),
    remove: true,
    redownload: download && isRemovable(s),
    queuePosition: download && s === "Queued, Remotely",
    deny: !download && !isRemovable(s),
    bump: !download && isQueued(s)
  };
};
