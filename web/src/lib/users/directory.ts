import type { Schemas } from "#lib/api/client.js";
import { type PageSizes, placeIn, withPlace } from "#lib/lists/pages.js";

/** the page sizes the directory offers, and the one it starts at */
export const DIRECTORY_PAGES: PageSizes = { sizes: [25, 50, 100, 200], size: 50 };

/** what the address holds of the directory: its filters and the page shown */
export type DirectoryAddress = {
  q: string;
  blocked: boolean;
  buddies: boolean;
  presence: Schemas["UserPresence"] | "";
  group: string;
  hasTransfers: boolean;
  page: number;
  size: number;
};

const PRESENCES: Schemas["UserPresence"][] = ["Online", "Away", "Offline"];

/**
 * The directory's part of a users address: `q`, `blocked=1`, `buddies=1`, `presence`, `group`,
 * `has=transfers`, and the page in `page` and `per_page`. What a link leaves out is off
 */
export const directoryAt = (query: URLSearchParams): DirectoryAddress => ({
  q: query.get("q")?.trim() ?? "",
  blocked: query.get("blocked") === "1",
  buddies: query.get("buddies") === "1",
  presence: PRESENCES.find((p) => p === query.get("presence")) ?? "",
  group: query.get("group") ?? "",
  hasTransfers: query.get("has") === "transfers",
  ...placeIn(query, DIRECTORY_PAGES)
});

/** `query` with the directory's part of it written as `at` says, what is off left out */
export const withDirectory = (query: URLSearchParams, at: DirectoryAddress) => {
  const next = withPlace(query, at, DIRECTORY_PAGES);
  const put = (name: string, value: string) => (value ? next.set(name, value) : next.delete(name));
  put("q", at.q);
  put("blocked", at.blocked ? "1" : "");
  put("buddies", at.buddies ? "1" : "");
  put("presence", at.presence);
  put("group", at.group);
  put("has", at.hasTransfers ? "transfers" : "");
  return next;
};
