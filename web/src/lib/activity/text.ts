import { plural } from "#lib/format.js";
import { endingText, isSucceeded } from "#lib/transfers/states.js";

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
 * A run of an entry's sentence: plain words, a user, a file, a room, or words set apart (the
 * search, the count, how an upload ended, its size)
 */
export type Part =
  | { kind: "text"; text: string }
  | { kind: "user"; name: string }
  | { kind: "file"; path: string }
  | { kind: "room"; name: string }
  | { kind: "strong" | "failed" | "muted"; text: string };

const t = (text: string): Part => ({ kind: "text", text });

/**
 * An entry's sentence as runs, the one place its words are: the dock renders the runs with the
 * names, files and rooms as links, and `activityText` joins them for copying and screen readers.
 * `size` writes a byte count
 */
export const activityParts = (e: ActivityEntry, size: (bytes: number) => string): Part[] => {
  const who: Part[] = e.username ? [{ kind: "user", name: e.username }] : [];
  const k = e.kind;
  switch (k.type) {
    case "browse":
      return [...who, t(" is browsing your shares")];
    case "profile":
      return [...who, t(" is viewing your profile")];
    case "search":
      return [
        ...who,
        t(" searched for "),
        { kind: "strong", text: `"${k.query}"` },
        { kind: "muted", text: ` (${plural(k.results, "result", "results")})` }
      ];
    case "queued":
      return [
        ...who,
        t(" queued "),
        { kind: "strong", text: plural(k.files, "file", "files") },
        { kind: "muted", text: ", the last " },
        { kind: "file", path: k.filename }
      ];
    case "uploadStarted":
      return [
        t("Uploading "),
        { kind: "file", path: k.filename },
        { kind: "muted", text: ` (${size(k.size)})` },
        t(" to "),
        ...who
      ];
    case "uploadEnded":
      return isSucceeded(k.state)
        ? [t("Finished uploading "), { kind: "file", path: k.filename }, t(" to "), ...who]
        : [
            t("Upload of "),
            { kind: "file", path: k.filename },
            t(" to "),
            ...who,
            t(" "),
            { kind: "failed", text: endingText(k.state) },
            { kind: "muted", text: ` after ${size(k.bytes)}` }
          ];
    case "privateRoom": {
      const room: Part = { kind: "room", name: k.room };
      return {
        added: [t("You were added to the private room "), room],
        removed: [t("You were removed from the private room "), room],
        operator: [t("You are now an operator of "), room],
        noLongerOperator: [t("You are no longer an operator of "), room]
      }[k.change];
    }
  }
};

/** An entry's sentence as plain text, for copying and for screen readers */
export const activityText = (e: ActivityEntry, size: (bytes: number) => string): string =>
  activityParts(e, size)
    .map((p) =>
      p.kind === "user" || p.kind === "room"
        ? p.name
        : p.kind === "file"
          ? baseName(p.path)
          : p.text
    )
    .join("");
