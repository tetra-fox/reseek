import type { ActivityEntry, ActivityType } from "./activity.svelte";

/** A file's name without its folders, from a path in either separator */
export const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** What each kind of entry is called in the dock's filter */
export const KINDS: { type: ActivityType; label: string }[] = [
  { type: "browse", label: "Browses" },
  { type: "profile", label: "Profiles" },
  { type: "search", label: "Searches" },
  { type: "queued", label: "Queued" },
  { type: "uploadStarted", label: "Uploads" },
  { type: "privateRoom", label: "Rooms" }
];

/** The filter an entry falls under: an upload's start and end are one */
export const kindOf = (type: ActivityType): ActivityType =>
  type === "uploadEnded" ? "uploadStarted" : type;

/**
 * An entry as a sentence: what its line says, for copying and for screen readers. The dock
 * shows the same words with the names and files as links
 */
export const activityText = (e: ActivityEntry): string => {
  const who = e.username ?? "";
  const k = e.kind;
  switch (k.type) {
    case "browse":
      return `${who} is browsing your shares`;
    case "profile":
      return `${who} is viewing your profile`;
    case "search":
      return `${who} searched for "${k.query}" and got ${k.results === 1 ? "1 result" : `${k.results} results`}`;
    case "queued":
      return `${who} queued ${k.files === 1 ? "1 file" : `${k.files} files`}, the last ${baseName(k.filename)}`;
    case "uploadStarted":
      return `Uploading ${baseName(k.filename)} to ${who}`;
    case "uploadEnded":
      return k.state === "Completed, Succeeded"
        ? `Finished uploading ${baseName(k.filename)} to ${who}`
        : `Upload of ${baseName(k.filename)} to ${who} ended: ${k.state.replace("Completed, ", "")}`;
    case "privateRoom":
      return {
        added: `You were added to the private room ${k.room}`,
        removed: `You were removed from the private room ${k.room}`,
        operator: `You are an operator of ${k.room} now`,
        noLongerOperator: `You are no longer an operator of ${k.room}`
      }[k.change];
  }
};
