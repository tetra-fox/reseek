import { SvelteMap } from "svelte/reactivity";

import { formatBytes, formatCount, plural } from "#lib/format.js";

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

/** The queue cell's text: the place among the queue's uploads, counted from 1 */
export const placeText = (s: UploadQueueStatus) =>
  s.position === null || s.position === undefined
    ? `Queued in the ${s.group} queue`
    : `Queued (#${formatCount(s.position + 1)} in the ${s.group} queue)`;

/** Why an upload waits, in a line per reason */
export const whyText = (s: UploadQueueStatus): string => {
  const lines = [
    `${formatCount(s.groupSlotsUsed)} of ${formatCount(s.groupSlots)} upload slots of the ${s.group} queue in use`,
    `${formatCount(s.globalSlotsUsed)} of ${formatCount(s.globalSlots)} upload slots in use overall`
  ];
  if (s.globalSlotsUsed >= s.globalSlots) lines.push("All upload slots are in use");
  if (s.userActive)
    lines.push(`This user already has ${plural(s.userActive, "active upload", "active uploads")}`);
  return lines.join("\n");
};

type Usage = UploadQueueStatus["limits"]["daily"]["files"];

const limited = (u: Usage): u is Usage & { limit: number } =>
  u.limit !== null && u.limit !== undefined;

/**
 * A user's use of their group's limits today and this week, for the group header, with whether
 * any is used up. Failures show where they have a limit
 */
export const quota = (s: UploadQueueStatus) => {
  const of = (u: Usage, unit: (n: number) => string) =>
    limited(u) ? `${unit(u.used)}/${unit(u.limit)}` : unit(u.used);
  const part = (name: string, w: UploadQueueStatus["limits"]["daily"]) => {
    const files = limited(w.files)
      ? `${of(w.files, formatCount)} files`
      : plural(w.files.used, "file", "files");
    const failures =
      w.failures && limited(w.failures) ? [`${of(w.failures, formatCount)} failed`] : [];
    return `${name} ${[files, of(w.bytes, formatBytes), ...failures].join(", ")}`;
  };
  const windows = [s.limits.daily, s.limits.weekly];
  const exhausted = windows.some((w) =>
    [w.files, w.bytes, w.failures].some((u) => !!u && limited(u) && u.used >= u.limit)
  );
  return {
    text: `${part("Today:", s.limits.daily)} · ${part("This week:", s.limits.weekly)}`,
    exhausted
  };
};
