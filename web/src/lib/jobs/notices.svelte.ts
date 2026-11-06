import { goto } from "$app/navigation";

import type { NotifyMode } from "#lib/chat/modes.js";
import { notifier, notifyOptions } from "#lib/chat/notify.svelte.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { href } from "#lib/nav.js";

import { endedBadly, ENDING_LABELS, type Job } from "./jobs";

/** A job's end as a notification: its name, then how it ended and what it came to */
export const jobNotice = (job: Job) => {
  const ending = job.ending ? ENDING_LABELS[job.ending].toLowerCase() : "ended";
  return {
    title: `${job.name}: ${ending}`,
    body: job.error ?? job.summary ?? ""
  };
};

/** The mode a job's end is told in, by its kind; download jobs are told by their own notice */
const modeOf = (job: Job): NotifyMode | null => {
  const options = notifyOptions();
  if (job.kind === "scan") return options.scansFinished;
  if (job.kind === "analysis") return options.analysisFinished;
  if (job.kind === "maintenance") return options.maintenanceFinished;
  return null;
};

/**
 * Tells of the end of a share scan, an analysis or a maintenance run, as each kind's option
 * says; a maintenance run that failed is shown even with its option off
 */
class JobNotices {
  #told = new Set<string>();

  constructor() {
    live.on("jobs.updated", (job) => this.#updated(job));
  }

  #updated(job: Job) {
    if (job.state !== "ended" || this.#told.has(job.id)) return;
    this.#told.add(job.id);
    const mode = modeOf(job);
    if (mode === null) return;
    const { title, body } = jobNotice(job);
    const to = href(`/jobs?job=${job.id}`);
    if (mode === "off") {
      if (job.kind === "maintenance" && endedBadly(job)) {
        toast(title, "error", { action: { label: "Show", run: () => void goto(to) } });
      }
      return;
    }
    notifier.other(mode, title, body, `job:${job.id}`, to);
  }
}

// the shell imports this module for the notices alone; nothing reads them
new JobNotices();
