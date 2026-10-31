import { type Scale, SCALES } from "#lib/analysis/analysis.js";
import { fields, stored } from "#lib/stored.svelte.js";

/**
 * How this browser shows spectrograms: the frequency scale the viewer last chose, null until
 * they choose one, which leaves the daemon's option to decide, and the volume they play at
 */
export const spectrogramView = stored("analysis.spectrogram", (saved) => {
  const { scale, volume } = fields(saved);
  return {
    scale: SCALES.find((s) => s.id === scale)?.id ?? (null as Scale | null),
    volume: typeof volume === "number" && volume >= 0 && volume <= 1 ? volume : 1
  };
});
