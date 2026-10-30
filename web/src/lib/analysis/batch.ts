import { api, type Schemas } from "#lib/api/client.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { getFileName, listText, plural } from "#lib/format.js";
import { call } from "#lib/request.js";
import { isSucceeded, type Transfer } from "#lib/transfers/states.js";

import type { Target } from "./analysis";
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
  /** kept past the queue's limit by the daemon, which queues them as the queue drains */
  following: number;
  /** downloads that have not succeeded */
  unfinished: number;
  /** files whose extension is not in the analysis extensions */
  otherType: number;
  /** files with a current report, or in the queue already */
  current: number;
  /** the files the daemon refused, each with its reason */
  failed: string[];
};

/** How many files one call names, so a body stays well under the daemon's size limit */
const CALL_FILES = 5000;

/** A target as the batch call names it */
const named = (t: Target): Schemas["AnalysisTarget"] =>
  t.kind === "file"
    ? { kind: "file", root: t.root, path: t.path }
    : { kind: "download", username: t.username, id: t.id };

/**
 * Queues every finished candidate whose extension the analyzer is offered for, in the order they
 * were given; the daemon keeps what its queue has no room for and queues it as the queue drains
 */
const queueBatch = async (candidates: Candidate[]): Promise<BatchResult> => {
  const result: BatchResult = {
    queued: 0,
    following: 0,
    unfinished: 0,
    otherType: 0,
    current: 0,
    failed: []
  };
  const sent: Candidate[] = [];
  for (const c of candidates) {
    if (!c.finished) result.unfinished++;
    else if (!analyzable(c.name)) result.otherType++;
    else sent.push(c);
  }
  for (let at = 0; at < sent.length; at += CALL_FILES) {
    const part = sent.slice(at, at + CALL_FILES);
    const outcome = await call(() =>
      api.POST("/api/v0/analysis/batch", {
        body: { files: part.map((c) => named(c.target)) }
      })
    );
    if (!outcome.ok) {
      result.failed.push(`${plural(part.length, "file", "files")}: ${outcome.error}`);
      continue;
    }
    result.queued += outcome.data.queued;
    result.following += outcome.data.following;
    result.current += outcome.data.current;
    for (const refusal of outcome.data.refused) {
      result.failed.push(`${part[refusal.index]?.name ?? "a file"}: ${refusal.error}`);
    }
  }
  return result;
};

/** A batch's result in words: what was queued and what follows, then what was skipped and why */
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
  const following = r.following
    ? `, ${r.following.toLocaleString()} more will follow as the queue drains`
    : "";
  return skipped
    ? `${queued}${following}. ${plural(skipped, "file", "files")} skipped: ${listText(reasons)}.`
    : `${queued}${following}`;
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
  if (result.queued || result.following || !result.failed.length) toast(batchSummary(result));
};
