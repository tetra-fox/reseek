import type { Schemas } from "#lib/api/client.js";
import { parseDuration, plural, sayDuration } from "#lib/format.js";

export type Job = Schemas["Job"];
export type JobKind = Job["kind"];
export type JobState = Job["state"];
export type JobEnding = NonNullable<Job["ending"]>;
export type StarterKind = Job["starter"]["kind"];
export type Schedule = Schemas["JobSchedule"];
export type JobPart = Schemas["JobPart"];

/** What each kind of job is called, as the filters and the rows name it */
export const KIND_LABELS: Record<JobKind, string> = {
  download: "Download",
  scan: "Share scan",
  analysis: "Analysis",
  maintenance: "Maintenance"
};

/** How each ending reads */
export const ENDING_LABELS: Record<JobEnding, string> = {
  succeeded: "Succeeded",
  finished_with_failures: "Finished with failures",
  failed: "Failed",
  canceled: "Canceled"
};

/** The states and endings the state filter offers, in its order */
export const STATE_FILTERS = [
  { id: "running", label: "Running" },
  { id: "paused", label: "Paused" },
  { id: "queued", label: "Queued" },
  { id: "succeeded", label: "Succeeded" },
  { id: "finished_with_failures", label: "Finished with failures" },
  { id: "failed", label: "Failed" },
  { id: "canceled", label: "Canceled" }
] as const;
export type StateFilter = (typeof STATE_FILTERS)[number]["id"];

/** The starters the starter filter offers */
export const STARTER_LABELS: Record<StarterKind, string> = {
  you: "You",
  schedule: "A schedule",
  download: "A download",
  resumed: "A restart"
};

/** Who started a job, as its row says it */
export const starterText = (job: Job, schedules: Schedule[] = []): string => {
  const { kind, name } = job.starter;
  if (kind === "schedule") {
    const title = schedules.find((s) => s.name === name)?.title ?? name ?? "a schedule";
    return `Started by the ${title.toLowerCase()} schedule`;
  }
  if (kind === "download") return "Started by its download";
  if (kind === "resumed") return "Started when reseek started";
  return "Started by you";
};

/** Whether a job is in the filter's state or ended that way */
export const inState = (job: Job, filter: StateFilter): boolean =>
  job.state === "ended" ? job.ending === filter : job.state === filter;

/** Whether a job ended in a way the user should look at */
export const endedBadly = (job: Job) =>
  job.ending === "failed" || job.ending === "finished_with_failures";

/** The tone a job's state or ending is shown in */
export const toneOf = (job: Job): "accent" | "success" | "warning" | "danger" | "neutral" => {
  if (job.state === "running") return "accent";
  if (job.state === "paused" || job.state === "queued") return "neutral";
  if (job.ending === "succeeded") return "success";
  if (job.ending === "finished_with_failures") return "warning";
  if (job.ending === "failed") return "danger";
  return "neutral";
};

/** A job's state or ending as its badge says it */
export const stateText = (job: Job): string => {
  if (job.state === "ended") return job.ending ? ENDING_LABELS[job.ending] : "Ended";
  return { running: "Running", paused: "Paused", queued: "Queued" }[job.state];
};

/** How far a job is in words: "7 of 12 files", with the failures, or nothing for a job without parts */
export const progressText = (job: Job): string => {
  const { total, succeeded, failed, canceled } = job.counts;
  if (total === 0) return "";
  const noun = job.kind === "maintenance" ? ["step", "steps"] : ["file", "files"];
  const done = succeeded + failed + canceled;
  const head =
    job.state === "ended"
      ? plural(total, noun[0], noun[1])
      : `${done.toLocaleString()} of ${plural(total, noun[0], noun[1])}`;
  const tails = [
    failed ? `${failed.toLocaleString()} failed` : "",
    canceled && job.state === "ended" ? `${canceled.toLocaleString()} canceled` : ""
  ].filter(Boolean);
  return [head, ...tails].join(", ");
};

/** How far a job is, from 0 to 1, from its counts or what it reports; null when it can't say */
export const fraction = (job: Job): number | null => {
  if (job.progress !== null && job.progress !== undefined) return job.progress;
  const { total, succeeded, failed, canceled } = job.counts;
  return total ? (succeeded + failed + canceled) / total : null;
};

/** The order of the list: running and paused, then queued oldest first, then ended newest first */
export const ordered = (jobs: Job[]): Job[] => {
  const rank = (j: Job) =>
    j.state === "running" || j.state === "paused" ? 0 : j.state === "queued" ? 1 : 2;
  return jobs.toSorted((a, b) => {
    const byRank = rank(a) - rank(b);
    if (byRank) return byRank;
    if (rank(a) === 2) return (b.endedAt ?? "").localeCompare(a.endedAt ?? "");
    return a.createdAt.localeCompare(b.createdAt);
  });
};

/** What the filters keep */
export type JobFilters = {
  kinds: JobKind[];
  states: StateFilter[];
  starters: StarterKind[];
  text: string;
};

/** The jobs the filters keep, every one when a filter is empty */
export const filtered = (jobs: Job[], f: JobFilters): Job[] => {
  const text = f.text.trim().toLowerCase();
  return jobs.filter(
    (j) =>
      (!f.kinds.length || f.kinds.includes(j.kind)) &&
      (!f.states.length || f.states.some((s) => inState(j, s))) &&
      (!f.starters.length || f.starters.includes(j.starter.kind)) &&
      (!text || j.name.toLowerCase().includes(text))
  );
};

/**
 * What the bottom bar says of the jobs, null when none runs or is paused; `short` leaves out the
 * word jobs, for a phone
 */
export const summaryText = (
  summary: Schemas["JobsSummary"] | undefined,
  short = false
): string | null => {
  if (!summary) return null;
  const running = short
    ? `${summary.running.toLocaleString()} running`
    : `${plural(summary.running, "job", "jobs")} running`;
  const parts = [
    summary.running ? running : "",
    summary.paused ? `${summary.paused.toLocaleString()} paused` : ""
  ].filter(Boolean);
  return parts.length ? parts.join(", ") : null;
};

/**
 * Whether the last failure is not the one the Jobs page last showed, `seen` being the daemon's
 * own time of it then
 */
export const failureUnseen = (summary: Schemas["JobsSummary"] | undefined, seen: string | null) =>
  !!summary?.lastFailedAt && summary.lastFailedAt !== seen;

/** How often a schedule runs, as its card says it */
export const everyText = (s: Schedule): string => {
  if (!s.every) return "Off";
  const [interval, at] = s.every.split(" at ");
  const ms = parseDuration(interval);
  const head = `Every ${ms === null ? interval : sayDuration(ms).replace(/^1 /, "")}`;
  return at ? `${head} at ${at}` : head;
};
