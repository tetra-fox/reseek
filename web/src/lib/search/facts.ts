import { formatSpeed } from "#lib/format.js";

import type { SearchResponse } from "./results";

/** What a response says of its user's uploads: "1.2 MB/s · free slot · 3 queued" */
export const userFacts = (response: SearchResponse) =>
  `${formatSpeed(response.uploadSpeed)} · ${response.hasFreeUploadSlot ? "free slot" : "no free slot"} · ${response.queueLength} queued`;
