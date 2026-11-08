import type { Schemas } from "#lib/api/client.js";
import { notifier, notifyOptions } from "#lib/chat/notify.svelte.js";
import { formatCount, fromWhom, getDirectoryName, getFileName, plural } from "#lib/format.js";
import { live } from "#lib/live/live.svelte.js";
import { href } from "#lib/nav.js";

import type { Transfer } from "./states";

/** How long uploads queued by one user are gathered into one notification */
export const QUEUED_GATHER_MS = 2_000;

/** A finished job's notification: its title and body */
const jobNotice = (job: Schemas["DownloadJobFinished"]) => {
  const who = fromWhom(job.usernames);
  if (job.fileCount === 1) {
    const title = job.succeededCount ? "Download finished" : "Download failed";
    return { title, body: `${getFileName(job.filename)} from ${who}` };
  }
  const outcomes = [
    job.failedCount ? `${formatCount(job.failedCount)} failed` : "",
    job.canceledCount ? `${formatCount(job.canceledCount)} canceled` : ""
  ].filter(Boolean);
  // a folder at the top of a share names the share, not what was picked from it
  const name = /[\\/]/.test(job.folder) ? getFileName(job.folder) : "";
  const ended = job.succeededCount ? "finished" : "failed";
  return {
    title: name ? `${name} ${ended}` : `Downloads ${ended}`,
    body: [`${plural(job.fileCount, "file", "files")} from ${who}`, ...outcomes].join(", ")
  };
};

/**
 * Turns transfer frames into notifications: everything queued in one go finished, once, and
 * another user queued uploads from us, those gathered per user for a moment
 */
class TransferNotices {
  #queued = new Map<string, string[]>();

  constructor() {
    live.on("transfers.job_finished", (job) => this.#finished(job));
    live.on("transfers.added", (t) => this.#added(t));
  }

  #finished(job: Schemas["DownloadJobFinished"]) {
    // a job canceled whole was ended by the user, who needs no telling
    if (job.canceledCount === job.fileCount) return;
    const { title, body } = jobNotice(job);
    notifier.other(notifyOptions().jobFinished, title, body, `job:${job.id}`, href("/downloads"));
  }

  #added(t: Transfer) {
    if (t.direction !== "Upload" || notifyOptions().uploadQueued === "off") return;
    const gathered = this.#queued.get(t.username);
    if (gathered) return void gathered.push(t.filename);
    this.#queued.set(t.username, [t.filename]);
    setTimeout(() => {
      const files = this.#queued.get(t.username) ?? [];
      this.#queued.delete(t.username);
      notifier.other(
        notifyOptions().uploadQueued,
        `${t.username} queued ${plural(files.length, "file", "files")}`,
        files.length === 1 ? getFileName(files[0]) : getFileName(getDirectoryName(files[0])),
        `queued:${t.username}`,
        href("/uploads")
      );
    }, QUEUED_GATHER_MS);
  }
}

// the shell imports this module for the notices alone; nothing reads them
new TransferNotices();
