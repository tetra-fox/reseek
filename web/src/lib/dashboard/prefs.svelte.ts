import { fields, stored } from "#lib/stored.svelte.js";

import { RANGES, type RangeLabel } from "./reports";

/** The dashboard's chosen range, kept in this browser */
export const dashboardPrefs = stored("reseek.dashboard", (saved) => {
  const range = fields(saved).range;
  return {
    range: (RANGES.find((r) => r.label === range)?.label ?? "30d") as RangeLabel
  };
});
