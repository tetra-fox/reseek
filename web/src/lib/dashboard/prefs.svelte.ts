import { fields, stored } from "#lib/stored.svelte.js";

import { type Direction, RANGES, type RangeLabel } from "./reports";
import { VIEWS } from "./series";

const DIRECTIONS: Direction[] = ["Upload", "Download"];

/** The dashboard's choices from what was saved, a default for whatever is not a valid one */
export const readPrefs = (saved: unknown) => {
  const { range, view, hidden } = fields(saved);
  const off: unknown[] = Array.isArray(hidden) ? hidden : [];
  const turnedOff = DIRECTIONS.filter((d) => off.includes(d));
  return {
    range: (RANGES.find((r) => r.label === range)?.label ?? "30d") as RangeLabel,
    view: VIEWS.find((v) => v.id === view)?.id ?? "size",
    /** the directions turned off in Transfers over time, never both */
    hidden: turnedOff.length < DIRECTIONS.length ? turnedOff : []
  };
};

/** The dashboard's chosen range and chart, kept in this browser */
export const dashboardPrefs = stored("reseek.dashboard", readPrefs);
