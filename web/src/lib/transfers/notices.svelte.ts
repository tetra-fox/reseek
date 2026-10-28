import { api } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { notifier } from "#lib/chat/notify.svelte.js";
import { getDirectoryName, getFileName, plural } from "#lib/format.js";
import { live } from "#lib/live/live.svelte.js";
import { href } from "#lib/nav.js";
import { call } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";

import type { Transfer } from "./states";

/** The `notifications.transfers` options in effect, their defaults where the options don't say */
export const transferNoticeOptions = () => {
  const n = fields(fields(fields(app.options).notifications).transfers);
  const flag = (value: unknown, otherwise: boolean) =>
    typeof value === "boolean" ? value : otherwise;
  return {
    downloadFinished: flag(n.download_finished, true),
    batchFinished: flag(n.batch_finished, true),
    uploadQueued: flag(n.upload_queued, false)
  };
};

/** How long uploads queued by one user are gathered into one notification */
export const QUEUED_GATHER_MS = 2_000;

/**
 * Turns transfer frames into notifications: a download finished, every download of a batch of
 * several (an album) finished, and another user queued uploads from us, those gathered per user for a
 * moment. Each transfer and batch is told of once
 */
class TransferNotices {
  #told = new Set<string>();
  #queued = new Map<string, string[]>();

  constructor() {
    live.on("transfers.updated", (t) => void this.#updated(t));
    live.on("transfers.added", (t) => this.#added(t));
    live.on("transfers.removed", (t) => this.#told.delete(t.id));
  }

  async #updated(t: Transfer) {
    if (t.direction !== "Download" || t.state !== "Completed, Succeeded" || this.#told.has(t.id)) {
      return;
    }
    this.#told.add(t.id);
    const options = transferNoticeOptions();
    if (options.downloadFinished) {
      notifier.other(
        "Download finished",
        `${getFileName(t.filename)} from ${t.username}`,
        `download:${t.id}`,
        href("/downloads")
      );
    }
    if (!options.batchFinished || !t.batchId || this.#told.has(`batch:${t.batchId}`)) return;
    const batch = await call(() =>
      api.GET("/api/v0/transfers/downloads/batches/{id}", { params: { path: { id: t.batchId! } } })
    );
    // a batch that can't be read is only not told of; its files were. A batch of one file is
    // that file, already told of
    if (!batch.ok || batch.data.fileCount < 2) return;
    if (batch.data.completedCount < batch.data.fileCount) return;
    if (this.#told.has(`batch:${t.batchId}`)) return;
    this.#told.add(`batch:${t.batchId}`);
    const failed = batch.data.failedCount ? `, ${batch.data.failedCount} failed` : "";
    notifier.other(
      "Album finished",
      `${getFileName(getDirectoryName(t.filename))} from ${t.username}: ${plural(batch.data.fileCount, "file", "files")}${failed}`,
      `batch:${t.batchId}`,
      href("/downloads")
    );
  }

  #added(t: Transfer) {
    if (t.direction !== "Upload" || !transferNoticeOptions().uploadQueued) return;
    const gathered = this.#queued.get(t.username);
    if (gathered) return void gathered.push(t.filename);
    this.#queued.set(t.username, [t.filename]);
    setTimeout(() => {
      const files = this.#queued.get(t.username) ?? [];
      this.#queued.delete(t.username);
      notifier.other(
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
