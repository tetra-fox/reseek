import { fileUrl } from "#lib/system/localfiles.js";
import { contentUrl } from "#lib/transfers/content.js";

import type { Target } from "./analysis";
import type { Track } from "./playback.svelte";

/** A file as its slot plays it */
export const trackOf = (t: Target, filename: string): Track => ({
  filename,
  url: (as) =>
    t.kind === "file" ? fileUrl(t.root, t.path, "inline", as) : contentUrl(t, "inline", as)
});
