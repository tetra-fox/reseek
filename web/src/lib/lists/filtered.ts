import { listText, plural } from "#lib/format.js";

/** A filter narrowing a list, as the list's summary names it and takes it off */
export type ActiveFilter = {
  /** the chip's key, the same for as long as the filter is on */
  id: string;
  /** what the chip reads: "Failed", "Matching “flac”" */
  label: string;
  /** turns this filter off and leaves the others */
  clear: () => void;
};

/** What a list holds, one and many: ["download", "downloads"] */
export type Noun = readonly [string, string];

const NUMBER = new Intl.NumberFormat();

/** The id of a list's text filter among its filters */
export const TEXT_FILTER = "text";

/** The filter a list's text box sets, while it holds anything; `clear` empties the box */
export const typedFilter = (text: string, clear: () => void): ActiveFilter[] =>
  text.trim() ? [{ id: TEXT_FILTER, label: `Matching “${text.trim()}”`, clear }] : [];

/**
 * How much of a list the filters leave: "Showing 12 of 340 downloads". A list that counts only
 * what matched, as one the daemon filters, says "12 users match"
 */
export const showing = (shown: number, total: number | null, noun: Noun) => {
  if (total === null) return `${plural(shown, ...noun)} ${shown === 1 ? "matches" : "match"}`;
  if (shown === total)
    return total === 1 ? `Showing the only ${noun[0]}` : `Showing all ${plural(total, ...noun)}`;
  return `Showing ${NUMBER.format(shown)} of ${plural(total, ...noun)}`;
};

/** Why a list that holds something shows nothing: "The filters hide all 340 downloads" */
export const hiding = (total: number | null, noun: Noun) => {
  if (total === null) return `No ${noun[1]} match the filters`;
  if (total === 1) return `The filters hide the only ${noun[0]}`;
  return `The filters hide all ${plural(total, ...noun)}`;
};

/** What a screen reader hears as the filters change */
export const announce = (filters: ActiveFilter[]) =>
  filters.length ? `Filters on: ${listText(filters.map((f) => f.label))}` : "Filters cleared";

export const clearAll = (filters: ActiveFilter[]) => {
  for (const f of filters) f.clear();
};

/**
 * Which of `count` chips takes focus once the one at `at` is removed: the one after it, which
 * moves into its place, else the one before; null when it was the only one
 */
export const focusAfter = (at: number, count: number): number | null => {
  if (count <= 1) return null;
  return at < count - 1 ? at + 1 : at - 1;
};
