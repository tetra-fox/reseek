import { api, type Schemas } from "#lib/api/client.js";
import { call } from "#lib/request.js";

export type EnqueueItem = Schemas["EnqueueDownloadBatchItem"];

/** What an enqueue came to, in words for the status next to the button and its toast */
export type Enqueued =
  | { kind: "done"; text: string }
  | { kind: "partial" | "none" | "failed"; text: string };

type Request = {
  username: string;
  files: EnqueueItem[];
  /** the search the files were found by */
  searchId?: string;
  /** a relative folder under the downloads directory that every file goes to */
  destination?: string;
};

/**
 * Enqueues files from one user as one batch. The batch exists even when files were refused, so
 * the outcome is read from its failures: none, some or every file
 */
export const enqueue = async ({
  username,
  files,
  searchId,
  destination
}: Request): Promise<Enqueued> => {
  const outcome = await call(() =>
    api.POST("/api/v0/transfers/downloads/batches", {
      body: {
        username,
        files,
        searchId: searchId ?? null,
        options: destination ? { destination } : undefined
      }
    })
  );
  if (!outcome.ok) {
    const text = outcome.status === 404 ? `${username} appears to be offline` : outcome.error;
    return { kind: "failed", text };
  }
  const { failures } = outcome.data;
  const why = failures.map((f) => `${f.filename}: ${f.message}`).join("; ");
  if (failures.length === 0) {
    return {
      kind: "done",
      text: `${files.length} ${files.length === 1 ? "file" : "files"} queued`
    };
  }
  if (failures.length >= files.length)
    return { kind: "none", text: `Couldn't queue anything: ${why}` };
  return {
    kind: "partial",
    text: `Queued ${files.length - failures.length} of ${files.length} files. Couldn't queue ${why}`
  };
};

/** The attributes of a file in a listing, as an enqueue keeps them to show with the download */
export const asItem = (file: Schemas["File"], filename = file.filename): EnqueueItem => ({
  filename,
  size: file.size,
  attributes: file.attributes
});
