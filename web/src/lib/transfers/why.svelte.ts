import { SvelteMap } from "svelte/reactivity";

import { plural } from "#lib/format.js";

import type { Transfer } from "./states";
import { type UploadQueueStatus, why } from "./transfers.svelte";

/** How long an answer is used before it is asked again */
const FRESH_MS = 10_000;

/**
 * Why queued uploads wait, asked when the user points at one and kept a few seconds, so moving
 * over a column doesn't ask once per pass
 */
class Waits {
  #answers = new SvelteMap<string, { status: UploadQueueStatus; at: number }>();
  #asking = new Set<string>();

  get(id: string): UploadQueueStatus | undefined {
    return this.#answers.get(id)?.status;
  }

  load = async (t: Transfer) => {
    const held = this.#answers.get(t.id);
    if ((held && Date.now() - held.at < FRESH_MS) || this.#asking.has(t.id)) return;
    this.#asking.add(t.id);
    const outcome = await why(t);
    this.#asking.delete(t.id);
    // an upload that left the queue has nothing to say; the tooltip stays empty
    if (outcome.ok) this.#answers.set(t.id, { status: outcome.data, at: Date.now() });
  };
}

export const waits = new Waits();

/** The queue cell's text: the place among the group's uploads, counted from 1 */
export const placeText = (s: UploadQueueStatus) =>
  s.position === null || s.position === undefined
    ? `Queued in group ${s.group}`
    : `Queued (#${s.position + 1} in group ${s.group})`;

/** Why an upload waits, in a line per reason */
export const whyText = (s: UploadQueueStatus): string => {
  const lines = [
    `${s.groupSlotsUsed} of ${s.groupSlots} upload slots for group ${s.group} in use`,
    `${s.globalSlotsUsed} of ${s.globalSlots} upload slots in use overall`
  ];
  if (s.globalSlotsUsed >= s.globalSlots) lines.push("All upload slots are in use");
  if (s.userActive)
    lines.push(`This user already has ${plural(s.userActive, "active upload", "active uploads")}`);
  return lines.join("\n");
};

/** A user's use of their group's limits, for the group header, with whether any is used up */
export const quota = (s: UploadQueueStatus) => {
  const mb = (n: number) => Math.round(n / 1_000_000);
  const part = (name: string, w: UploadQueueStatus["limits"]["daily"]) => {
    const files = w.files.limit
      ? `${w.files.used}/${w.files.limit} files`
      : plural(w.files.used, "file", "files");
    const bytes = w.bytes.limit
      ? `${mb(w.bytes.used)}/${mb(w.bytes.limit)} MB`
      : `${mb(w.bytes.used)} MB`;
    return `${name} ${files}, ${bytes}`;
  };
  const windows = [s.limits.daily, s.limits.weekly];
  const exhausted = windows.some(
    (w) =>
      (w.files.limit !== null && w.files.limit !== undefined && w.files.used >= w.files.limit) ||
      (w.bytes.limit !== null && w.bytes.limit !== undefined && w.bytes.used >= w.bytes.limit)
  );
  return {
    text: `${part("Today:", s.limits.daily)} · ${part("This week:", s.limits.weekly)}`,
    exhausted
  };
};
