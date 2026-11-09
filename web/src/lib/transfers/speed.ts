import { api, type Schemas } from "#lib/api/client.js";
import { formatCount } from "#lib/format.js";
import { call, type Outcome } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";

/** A total speed limit of each direction in KiB/s, null for none */
export type LimitPair = { download: number | null; upload: number | null };

/** Both pairs of speed limits and which one is in force */
export type SpeedLimits = { alternative: boolean; normal: LimitPair; alt: LimitPair };

const limit = (value: unknown): number | null =>
  typeof value === "number" && value > 0 ? value : null;

/** Both pairs in the options in effect, and which one the state says is in force */
export const speedLimitsOf = (
  options: Schemas["OptionsDocument"] | null,
  state: Schemas["State"] | null
): SpeedLimits => {
  const transfers = fields(fields(options).transfers);
  const download = fields(transfers.download);
  const upload = fields(transfers.upload);
  return {
    alternative: state?.alternativeSpeedLimits ?? false,
    normal: { download: limit(download.speed_limit), upload: limit(upload.speed_limit) },
    alt: { download: limit(download.alt_speed_limit), upload: limit(upload.alt_speed_limit) }
  };
};

/** A limit as the footer names it */
export const formatLimit = (kib: number | null): string =>
  kib === null ? "No limit" : `${formatCount(kib)} KiB/s`;

/** Puts the alternative limits in force, or the normal ones; the state frame brings it back */
export const switchSpeedLimits = (alternative: boolean): Promise<Outcome<unknown>> =>
  call(() => api.PUT("/api/v0/transfers/speed-limits", { body: { alternative } }));
