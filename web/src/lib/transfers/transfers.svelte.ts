import { SvelteMap, SvelteSet } from "svelte/reactivity";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import { loadFailed, toast } from "#lib/feedback/toast.svelte.js";
import { undoable } from "#lib/feedback/undo.js";
import { plural } from "#lib/format.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { type BulkAction, summary } from "./bulk";
import type { Direction, Transfer } from "./states";

/** How many speed samples a row's sparkline keeps */
export const SPARK_POINTS = 20;

const NAMES = { download: "Download", upload: "Upload" } as const;

/** What finds a transfer of one user's file: the user and the file's full remote path */
export const fileKey = (username: string, path: string) => `${username}\u0000${path}`;

/**
 * One direction's transfers, kept by id: frames on the `transfers` topic add, patch and drop
 * rows, and a reload reconciles them with the grouped listing. The last speeds each row reported
 * are kept for its sparkline
 */
export class Transfers {
  readonly direction: Direction;
  list = new Keyed<Transfer>((t) => t.id);
  loaded = $state(false);
  /** why the last load failed, empty once one succeeds */
  error = $state("");
  speeds = new SvelteMap<string, number[]>();
  /** why the last ask for a download's place in the queue failed, by transfer id */
  queueErrors = new SvelteMap<string, string>();
  /** transfers removed on screen whose removal waits until it can no longer be undone */
  hidden = new SvelteSet<string>();
  /**
   * Each transfer by `fileKey`, the newest when a file was transferred more than once, leaving
   * out those removed on screen. Made again only when a transfer comes, goes or is hidden: a
   * transfer's progress patches the row it finds in place
   */
  byFile = $derived(
    new Map(
      this.list.rows
        .filter((t) => !this.hidden.has(t.id))
        .map((t) => [fileKey(t.username, t.filename), t])
    )
  );

  constructor(direction: Direction) {
    this.direction = direction;
    const mine = (t: { direction: string }) => t.direction === NAMES[direction];
    live.on("transfers.added", (t) => mine(t) && void this.list.upsert(t));
    live.on("transfers.updated", (t) => {
      if (!mine(t)) return;
      this.list.upsert(t);
      if (t.state === "InProgress") {
        this.speeds.set(
          t.id,
          [...(this.speeds.get(t.id) ?? []), t.averageSpeed].slice(-SPARK_POINTS)
        );
      }
    });
    live.on("transfers.removed", (t) => {
      if (!mine(t)) return;
      this.list.remove(t.id);
      this.hidden.delete(t.id);
      this.speeds.delete(t.id);
      this.queueErrors.delete(t.id);
    });
  }

  load = async () => {
    const turn = this.list.reading();
    const outcome =
      this.direction === "download"
        ? await call(() => api.GET("/api/v0/transfers/downloads"))
        : await call(() => api.GET("/api/v0/transfers/uploads"));
    if (!outcome.ok) return this.list.abandon(turn) ? loadFailed(this, outcome.error) : undefined;
    const files = outcome.data.flatMap((u) => u.directories.flatMap((d) => d.files));
    if (!this.list.reconcile(files, turn)) return;
    this.loaded = true;
    this.error = "";
  };

  /** One call for `action` on `ids`, answered with a line saying what it did and how many failed */
  bulk = async (
    action: BulkAction,
    ids: string[]
  ): Promise<Outcome<{ text: string; failed: number }>> => {
    const body = { action, ids };
    const outcome =
      this.direction === "download"
        ? await call(() => api.POST("/api/v0/transfers/downloads/bulk", { body }))
        : await call(() => api.POST("/api/v0/transfers/uploads/bulk", { body }));
    if (!outcome.ok) return outcome;
    return {
      ok: true,
      data: { text: summary(action, outcome.data), failed: outcome.data.failed.length }
    };
  };

  /**
   * Removes transfers once their Undo has passed, canceling running ones then; until then they
   * only leave the screen. `all` removes every completed one with one call instead
   */
  removeLater = (ids: string[], { all = false } = {}) => {
    if (!ids.length) return;
    for (const id of ids) this.hidden.add(id);
    const text = `Removed ${plural(ids.length, "transfer", "transfers")}`;
    undoable(
      text,
      async () => {
        // removed rows stay hidden until their frames take them off the list; a failure shows
        // them again
        if (all) {
          const outcome = await this.removeCompleted();
          if (outcome.ok) return;
          for (const id of ids) this.hidden.delete(id);
          toast(outcome.error, "error");
          return;
        }
        const outcome = await this.bulk("remove", ids);
        if (outcome.ok && !outcome.data.failed) return;
        for (const id of ids) this.hidden.delete(id);
        void this.load();
        toast(outcome.ok ? outcome.data.text : outcome.error, "error");
      },
      () => {
        for (const id of ids) this.hidden.delete(id);
      }
    );
  };

  /** Removes every completed transfer of this direction from the list */
  removeCompleted = () =>
    this.direction === "download"
      ? call(() => api.DELETE("/api/v0/transfers/downloads/all/completed"))
      : call(() => api.DELETE("/api/v0/transfers/uploads/all/completed"));

  /** Asks the peer for a queued download's place, and shows it on the row at once */
  placeInQueue = async (t: Transfer): Promise<Outcome<number>> => {
    const outcome = await call(() =>
      api.GET("/api/v0/transfers/downloads/{username}/{id}/position", {
        params: { path: { username: t.username, id: t.id } }
      })
    );
    const row = this.list.get(t.id);
    if (outcome.ok && row) row.placeInQueue = outcome.data;
    if (outcome.ok) this.queueErrors.delete(t.id);
    else this.queueErrors.set(t.id, outcome.error);
    return outcome;
  };

  /** One transfer with the states it went through */
  history = (t: Transfer) =>
    this.direction === "download"
      ? call(() =>
          api.GET("/api/v0/transfers/downloads/{username}/{id}", {
            params: { path: { username: t.username, id: t.id }, query: { includeHistory: true } }
          })
        )
      : call(() =>
          api.GET("/api/v0/transfers/uploads/{username}/{id}", {
            params: { path: { username: t.username, id: t.id }, query: { includeHistory: true } }
          })
        );

  clear() {
    this.list.clear();
    this.speeds.clear();
    this.queueErrors.clear();
    this.hidden.clear();
    this.loaded = false;
  }
}

/** Why a queued upload waits: its place, its group's slots and the user's limits */
export const why = (t: Transfer) =>
  call(() =>
    api.GET("/api/v0/transfers/uploads/{username}/{id}/why", {
      params: { path: { username: t.username, id: t.id } }
    })
  );

/** Moves a queued upload to the front of its group for the next free slot */
export const bump = (t: Transfer) =>
  call(() =>
    api.POST("/api/v0/transfers/uploads/{username}/{id}/bump", {
      params: { path: { username: t.username, id: t.id } }
    })
  );

/** Where a user's endpoint is, asked of the server */
export const endpoint = (username: string) =>
  call(() => api.GET("/api/v0/users/{username}/endpoint", { params: byUser(username) }));

export type UploadQueueStatus = Schemas["UploadQueueStatus"];

export const downloads = new Transfers("download");
export const uploads = new Transfers("upload");
