import { SvelteMap, SvelteSet } from "svelte/reactivity";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import { fitsKeepalive } from "#lib/autosave.svelte.js";
import { loadFailed, toast } from "#lib/feedback/toast.svelte.js";
import { undoable } from "#lib/feedback/undo.js";
import { plural } from "#lib/format.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { type BulkAction, summary } from "./bulk";
import { type Direction, isEnded, isMoving, type Transfer } from "./states";
import { unseen } from "./unseen.svelte";

/** How many speed samples a row's sparkline keeps */
export const SPARK_POINTS = 20;

const NAMES = { download: "Download", upload: "Upload" } as const;

/** What finds a transfer of one user's file: the user and the file's full remote path */
export const fileKey = (username: string, path: string) => `${username}\u0000${path}`;

/**
 * One direction's transfers, kept by id: frames on the `transfers` topic add, patch and drop
 * rows, and a reload reconciles them with the grouped listing. Once read, it reads again when
 * frames were missed, as the topic sends no snapshot. The last speeds each row reported are kept
 * for its sparkline
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
   * The place of each download waiting in the local queue, 1 first, by id; set from the
   * daemon's order when it is read or moved, so downloads queued since come after
   */
  places = $state.raw(new Map<string, number>());
  /** counts the queue orders taken from frames, which are newer than any answer on its way */
  #orderFrames = 0;
  /** counts the queue orders taken from frames and from answers to moves */
  #orderWrites = 0;
  /** counts the reads of the queue order begun, so only the last one's answer is taken */
  #orderReads = 0;
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
      if (isMoving(t.state)) {
        this.speeds.set(
          t.id,
          [...(this.speeds.get(t.id) ?? []), t.averageSpeed].slice(-SPARK_POINTS)
        );
      }
    });
    live.on("transfers.queue", ({ ids }) => {
      this.#orderFrames += 1;
      this.#orderWrites += 1;
      this.#ordered(ids);
    });
    live.on("transfers.removed", (t) => {
      if (!mine(t)) return;
      this.list.remove(t.id);
      this.hidden.delete(t.id);
      this.speeds.delete(t.id);
      this.queueErrors.delete(t.id);
    });
    live.onMissed(() => {
      if (this.loaded) void this.load();
    });
  }

  #ordered(ids: string[]) {
    this.places = new Map(ids.map((id, i) => [id, i + 1]));
  }

  /**
   * Moves waiting downloads to start just before `before`, or last when it is null. The answer
   * gives the order, unless a frame gave one since, which is as new or newer
   */
  move = async (ids: string[], before: string | null) => {
    const frames = this.#orderFrames;
    const outcome = await call(() =>
      api.POST("/api/v0/transfers/downloads/queue", { body: { ids, before } })
    );
    if (!outcome.ok) {
      toast(outcome.error, "error");
      return;
    }
    if (frames !== this.#orderFrames) return;
    this.#orderWrites += 1;
    this.#ordered(outcome.data.ids);
  };

  /** Reads the queue order, taken unless a later read began or a frame or a move gave one since */
  #readOrder = async () => {
    const read = ++this.#orderReads;
    const writes = this.#orderWrites;
    const order = await call(() => api.GET("/api/v0/transfers/downloads/queue"));
    if (read !== this.#orderReads) return;
    if (!order.ok) toast(`Could not load the queue order: ${order.error}`, "error");
    else if (writes === this.#orderWrites) this.#ordered(order.data.ids);
  };

  load = async () => {
    if (this.direction === "download") void this.#readOrder();
    const turn = this.list.reading();
    const outcome =
      this.direction === "download"
        ? await call(() => api.GET("/api/v0/transfers/downloads"))
        : await call(() => api.GET("/api/v0/transfers/uploads"));
    if (!outcome.ok) return this.list.abandon(turn) ? loadFailed(this, outcome.error) : undefined;
    const files = outcome.data.flatMap((u) => u.directories.flatMap((d) => d.files));
    // what ends between frames, as while they were missed, counts toward the navigation's badges
    const open = this.list.rows.filter((t) => !isEnded(t.state)).map((t) => t.id);
    if (!this.list.reconcile(files, turn)) return;
    for (const id of open) {
      const t = this.list.get(id);
      if (t) unseen.count(t);
    }
    this.loaded = true;
    this.error = "";
  };

  /**
   * One call for `action` on `ids`, answered with a line saying what it did and how many failed.
   * With `keepalive` the request outlives the page, when its body fits what a browser sends so;
   * a larger one goes as a plain request, which the page may not outlive
   */
  bulk = async (
    action: BulkAction,
    ids: string[],
    keepalive = false
  ): Promise<Outcome<{ text: string; failed: number }>> => {
    const body = { action, ids };
    const init = { body, keepalive: keepalive && fitsKeepalive(body) };
    const outcome =
      this.direction === "download"
        ? await call(() => api.POST("/api/v0/transfers/downloads/bulk", init))
        : await call(() => api.POST("/api/v0/transfers/uploads/bulk", init));
    if (!outcome.ok) return outcome;
    return {
      ok: true,
      data: { text: summary(action, outcome.data), failed: outcome.data.failed.length }
    };
  };

  /**
   * Removes transfers once their Undo has passed, canceling running ones then; until then they
   * only leave the screen. Only the transfers named go, whatever finished meanwhile
   */
  removeLater = (ids: string[]) => {
    if (!ids.length) return;
    for (const id of ids) this.hidden.add(id);
    const text = `Removed ${plural(ids.length, "transfer", "transfers")}`;
    undoable(
      text,
      async (leaving) => {
        // removed rows stay hidden until their frames take them off the list; a failure shows
        // them again
        const outcome = await this.bulk("remove", ids, leaving);
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

  /** Asks the peer for a queued download's place, and shows it on the row at once */
  placeInQueue = async (t: Transfer): Promise<Outcome<number>> => {
    // a frame for the row while the peer is asked is newer than the answer
    const turn = this.list.readingRow();
    const outcome = await call(() =>
      api.GET("/api/v0/transfers/downloads/{username}/{id}/position", {
        params: { path: { ...byUser(t.username).path, id: t.id }, query: byUser(t.username).query }
      })
    );
    if (outcome.ok) {
      this.list.assignRow(t.id, { placeInQueue: outcome.data }, turn);
      this.queueErrors.delete(t.id);
    } else {
      this.list.dropRow();
      this.queueErrors.set(t.id, outcome.error);
    }
    return outcome;
  };

  /** One transfer with the states it went through */
  history = (t: Transfer) =>
    this.direction === "download"
      ? call(() =>
          api.GET("/api/v0/transfers/downloads/{username}/{id}", {
            params: {
              path: { ...byUser(t.username).path, id: t.id },
              query: { ...byUser(t.username).query, includeHistory: true }
            }
          })
        )
      : call(() =>
          api.GET("/api/v0/transfers/uploads/{username}/{id}", {
            params: {
              path: { ...byUser(t.username).path, id: t.id },
              query: { ...byUser(t.username).query, includeHistory: true }
            }
          })
        );

  clear() {
    this.list.clear();
    this.speeds.clear();
    this.queueErrors.clear();
    this.hidden.clear();
    this.places = new Map();
    // an order on its way belongs to the session that ended
    this.#orderReads += 1;
    this.#orderFrames += 1;
    this.loaded = false;
  }
}

/** Why a queued upload waits: its place, its group's slots and the user's limits */
export const why = (t: Transfer) =>
  call(() =>
    api.GET("/api/v0/transfers/uploads/{username}/{id}/why", {
      params: { path: { ...byUser(t.username).path, id: t.id }, query: byUser(t.username).query }
    })
  );

/** Moves a queued upload to the front of its group for the next free slot */
export const bump = (t: Transfer) =>
  call(() =>
    api.POST("/api/v0/transfers/uploads/{username}/{id}/bump", {
      params: { path: { ...byUser(t.username).path, id: t.id }, query: byUser(t.username).query }
    })
  );

/** Where a user's endpoint is, asked of the server */
export const endpoint = (username: string) =>
  call(() => api.GET("/api/v0/users/{username}/endpoint", { params: byUser(username) }));

export type UploadQueueStatus = Schemas["UploadQueueStatus"];

export const downloads = new Transfers("download");
export const uploads = new Transfers("upload");
