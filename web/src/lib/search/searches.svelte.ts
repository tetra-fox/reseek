import { api, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

export type Search = Schemas["Search"];
export type SearchScope = Schemas["SearchScope"];

/** A search's state names the terminal flag once it is done */
export const isDone = (state: string) => state.includes("Completed");

/** The scope as the list shows it, null for the whole network */
export const scopeLabel = (scope: SearchScope): string | null =>
  scope.type === "network" ? null : `${scope.type}: ${(scope.subjects ?? []).join(", ")}`;

/**
 * The searches, kept by id: the `search` topic's snapshot sets them, its frames patch them, and
 * a reload reconciles them with `GET /searches`
 */
class Searches {
  list = new Keyed<Search>((s) => s.id);
  /** a snapshot or a load has filled the list */
  loaded = $state(false);
  error = $state("");

  constructor() {
    live.on("search.list", (rows) => this.#set(rows));
    live.on("search.create", (row) => void this.list.upsert(row));
    live.on("search.update", (row) => void this.list.upsert(row));
    live.on("search.delete", (row) => this.list.remove(row.id));
  }

  load = async () => {
    const outcome = await call(() => api.GET("/api/v0/searches"));
    if (outcome.ok) this.#set(outcome.data);
    else this.error = outcome.error;
  };

  /** Starts a search with a fresh id, so its page can open before any frame arrives */
  create = async (text: string, scope?: SearchScope): Promise<Outcome<Search>> => {
    const outcome = await call(() =>
      api.POST("/api/v0/searches", {
        body: { id: crypto.randomUUID(), searchText: text, scope }
      })
    );
    if (outcome.ok) this.list.upsert(outcome.data);
    return outcome;
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

  clear() {
    this.list.clear();
    this.loaded = false;
  }

  #set(rows: Search[]) {
    this.list.reconcile(rows);
    this.loaded = true;
    this.error = "";
  }
}

export const searches = new Searches();
