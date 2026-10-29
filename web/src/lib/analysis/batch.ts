import { getFileName, listText, plural } from "#lib/format.js";
import { toast } from "#lib/toast.svelte.js";
import { isSucceeded, type Transfer } from "#lib/transfers/states.js";

import { queue, type Target } from "./analysis";
import { jobs } from "./jobs.svelte";
import { analyzable } from "./marks.svelte";

/** A file offered to a batch analysis: what names it, its name, and whether it is there whole */
export type Candidate = { target: Target; name: string; finished: boolean };

/** A download as a candidate: its file is there whole once the download has succeeded */
export const downloadCandidate = (t: Transfer): Candidate => ({
  target: { kind: "download", username: t.username, id: t.id },
  name: getFileName(t.filename),
  finished: isSucceeded(t.state)
});

/** Whether a download has succeeded with a file the analyzer is offered for */
export const analyzableDownload = (t: Transfer) =>
  isSucceeded(t.state) && analyzable(getFileName(t.filename));

/** What queuing a batch came to: how many were queued and why the rest were not */
type BatchResult = {
  queued: number;
  /** downloads that have not succeeded */
  unfinished: number;
  /** files whose extension is not in the analysis extensions */
  otherType: number;
  /** files with a current report, or in the queue already */
  current: number;
  /** the files the daemon refused otherwise, each with its reason */
  failed: string[];
};

/**
 * Queues every finished candidate whose extension the analyzer is offered for, one call each in
 * turn, so the queue keeps the order they were given in
 */
const queueBatch = async (candidates: Candidate[]): Promise<BatchResult> => {
  const result: BatchResult = { queued: 0, unfinished: 0, otherType: 0, current: 0, failed: [] };
  for (const c of candidates) {
    if (!c.finished) {
      result.unfinished++;
      continue;
    }
    if (!analyzable(c.name)) {
      result.otherType++;
      continue;
    }
    const outcome = await queue(c.target);
    if (outcome.ok) result.queued++;
    // 409: analyzed already with a current report, or queued already
    else if (outcome.status === 409) result.current++;
    else result.failed.push(`${c.name}: ${outcome.error}`);
  }
  return result;
};

/** A batch's result in words: what was queued, then what was skipped and why */
const batchSummary = (r: BatchResult): string => {
  const reasons = [
    r.unfinished ? `${r.unfinished} not finished` : "",
    r.otherType ? `${r.otherType} not an audio type` : "",
    r.current ? `${r.current} already analyzed or queued` : ""
  ].filter(Boolean);
  const skipped = r.unfinished + r.otherType + r.current;
  const queued = r.queued
    ? `${plural(r.queued, "file", "files")} queued for analysis`
    : "No files queued for analysis";
  return skipped
    ? `${queued}. ${plural(skipped, "file", "files")} skipped: ${listText(reasons)}.`
    : queued;
};

/**
 * Queues a batch and says in a toast what it came to; `empty` is said instead when there is
 * nothing in it at all
 */
export const analyzeBatch = async (candidates: Candidate[], empty: string) => {
  if (!candidates.length) return void toast(empty);
  const result = await queueBatch(candidates);
  void jobs.refresh();
  if (result.failed.length) toast(`Could not queue ${result.failed.join("; ")}`, "error");
  if (result.queued || !result.failed.length) toast(batchSummary(result));
};
