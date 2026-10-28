import { SvelteMap, SvelteSet } from "svelte/reactivity";

import { api, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";
import { toast } from "#lib/toast.svelte.js";
import { undoable } from "#lib/undo.js";

export type Search = Schemas["Search"];
export type SearchScope = Schemas["SearchScope"];

/** A search's state names the terminal flag once it is done */
export const isDone = (state: string) => state.includes("Completed");

/** The scope as the list shows it, null for the whole network */
export const scopeLabel = (scope: SearchScope): string | null =>
  scope.type === "network" ? null : `${scope.type}: ${(scope.subjects ?? []).join(", ")}`;

/** The row a search has while the daemon is asked to start it */
const placeholder = (id: string, text: string, scope?: SearchScope): Search => ({
  id,
  searchText: text,
  token: 0,
  state: "Requested",
  isComplete: false,
  startedAt: new Date().toISOString(),
  endedAt: null,
  responseCount: 0,
  fileCount: 0,
  lockedFileCount: 0,
  responseBytes: 0,
  responses: null,
  scope: scope ?? { type: "network", subjects: [] }
});

/**
 * The searches, kept by id: the `search` topic's snapshot sets them, its frames patch them, and
 * a reload reconciles them with `GET /searches`
 */
class Searches {
  list = new Keyed<Search>((s) => s.id);
  /** a snapshot or a load has filled the list */
  loaded = $state(false);
  /** why the last load failed, empty once one succeeds */
  error = $state("");
  /** searches asked for whose answer hasn't come, so their page waits rather than looks them up */
  starting = new SvelteSet<string>();
  /** why a search could not start, by its id, for its page to say */
  refused = new SvelteMap<string, string>();
  /** searches deleted on screen whose delete waits until it can no longer be undone */
  hidden = new SvelteSet<string>();

  constructor() {
    live.on("search.list", (rows) => this.#set(rows));
    live.on("search.create", (row) => void this.list.upsert(row));
    live.on("search.update", (row) => void this.list.upsert(row));
    live.on("search.delete", (row) => this.list.remove(row.id));
  }

  load = async () => {
    const outcome = await call(() => api.GET("/api/v0/searches"));
    if (outcome.ok) return this.#set(outcome.data);
    // a failed first load is the page's to show; a failed reload says so once per outage
    if (this.loaded && !this.error) toast(outcome.error, "error");
    this.error = outcome.error;
  };

  /**
   * Starts a search and gives its id at once, so its page can open before the daemon answers; a
   * row stands for it until then. A refused search leaves the list and its reason is kept
   */
  start = (text: string, scope?: SearchScope): { id: string; done: Promise<Outcome<Search>> } => {
    const id = crypto.randomUUID();
    this.starting.add(id);
    this.list.upsert(placeholder(id, text, scope));
    const done = call(() =>
      api.POST("/api/v0/searches", { body: { id, searchText: text, scope } })
    ).then((outcome) => {
      this.starting.delete(id);
      if (outcome.ok) {
        this.list.upsert(outcome.data);
      } else {
        this.list.remove(id);
        this.refused.set(id, outcome.error);
      }
      return outcome;
    });
    return { id, done };
  };

  /** Fetches one search into the list, for a page opened before its frame arrived */
  fetch = async (id: string): Promise<Outcome<Search>> => {
    const outcome = await call(() =>
      api.GET("/api/v0/searches/{id}", { params: { path: { id } } })
    );
    if (outcome.ok) this.list.upsert(outcome.data);
    return outcome;
  };

  stop = (id: string) => call(() => api.PUT("/api/v0/searches/{id}", { params: { path: { id } } }));

  remove = async (id: string): Promise<Outcome<unknown>> => {
    const outcome = await call(() =>
      api.DELETE("/api/v0/searches/{id}", { params: { path: { id } } })
    );
    if (outcome.ok) this.list.remove(id);
    return outcome;
  };

  /**
   * Deletes searches once their Undo has passed; until then they only leave the screen. One
   * that can't be deleted comes back and says why
   */
  removeLater = (ids: string[]) => {
    if (!ids.length) return;
    for (const id of ids) this.hidden.add(id);
    const one = this.list.get(ids[0]);
    const text =
      ids.length === 1 && one ? `Deleted '${one.searchText}'` : `Deleted ${ids.length} searches`;
    undoable(
      text,
      async () => {
        for (const id of ids) {
          const outcome = await this.remove(id);
          this.hidden.delete(id);
          if (!outcome.ok) toast(outcome.error, "error");
        }
      },
      () => {
        for (const id of ids) this.hidden.delete(id);
      }
    );
  };

  clear() {
    this.list.clear();
    this.hidden.clear();
    this.refused.clear();
    this.loaded = false;
  }

  #set(rows: Search[]) {
    // a search still being asked for isn't in the daemon's list yet, and keeps its row
    const asked = [...this.starting].flatMap((id) => {
      const row = this.list.get(id);
      return row && !rows.some((r) => r.id === id) ? [row] : [];
    });
    this.list.reconcile([...rows, ...asked]);
    this.loaded = true;
    this.error = "";
  }
}

export const searches = new Searches();
