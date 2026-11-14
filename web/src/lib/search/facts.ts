import { badgeCount, formatCount, formatSpeed } from "#lib/format.js";
import type { Field } from "#lib/inspector/inspector.svelte.js";

import { QUEUE_MOST } from "./list";
import type { ResultFile, SearchResponse } from "./results";

/** A result's file as the inspector shows it, with what its user offers */
export type Picked = { file: ResultFile; username: string; facts: Field[] };

/**
 * What a response says of its user's uploads: "1.2 MB/s · free slot · 3 queued", with a queue as
 * long as the list's column writes it
 */
export const userFacts = (response: SearchResponse) =>
  `${formatSpeed(response.uploadSpeed)} · ${response.hasFreeUploadSlot ? "free slot" : "no free slot"} · ${badgeCount(response.queueLength, QUEUE_MOST)} queued`;

/** What a user offers with a file, as the inspector lists it */
export const offerFields = (speed: number, free: boolean, queued: number): Field[] => [
  { label: "Upload speed", value: formatSpeed(speed) },
  { label: "Free slot", value: free ? "Yes" : "No" },
  { label: "Queued", value: formatCount(queued) }
];
