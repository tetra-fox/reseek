import { SvelteMap, SvelteSet } from "svelte/reactivity";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import type { paths } from "#lib/api/schema.js";
import { patch } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

export type UserSummary = Schemas["UserSummary"];

/** Which users of the directory to read and in what order; the pages are the reader's */
type DirectoryQuery = Omit<
  NonNullable<paths["/api/v0/users"]["get"]["parameters"]["query"]>,
  "offset" | "limit"
>;

/** How long a fetched summary is used before it is fetched again */
export const SUMMARY_TTL_MS = 60_000;

type Entry = { summary: UserSummary; at: number };

/**
 * What the directory says about users, by username, for the username menu and hover card:
 * fetched on first need from stored data only (never a Soulseek round trip), kept a minute, and
 * patched by `users.updated` frames while a screen follows the `users` topic. Every read and
 * every frame takes a number in turn, and a summary is taken only over one with a lower number,
 * so an answer read before a frame or before a later read does not undo it
 */
class Users {
  #entries = new SvelteMap<string, Entry>();
  #loading = new Map<string, Promise<void>>();
  /** the last number taken by a read or a frame */
  #turn = 0;
  /** the number of what each user's summary was last taken from */
  #taken = new Map<string, number>();
  /** the last number taken before everything was forgotten */
  #cleared = 0;
  /** our buddies' names, read by `loadBuddies` and kept by every summary that comes in */
  readonly buddies = new SvelteSet<string>();
  #buddiesAt = -Infinity;
  #buddiesLoading: Promise<void> | null = null;

  constructor() {
    live.on("users.updated", (summary) => this.#put(summary, ++this.#turn));
    live.on("users.removed", ({ username }) => {
      this.#taken.set(username, ++this.#turn);
      this.#entries.delete(username);
      this.buddies.delete(username);
    });
  }

  /** The summary held for `username`, whatever its age */
  get(username: string): UserSummary | undefined {
    return this.#entries.get(username)?.summary;
  }

  /**
   * Fetches the summary unless one younger than a minute is held, or `fresh` asks for it anyway.
   * A fetch already on its way is joined, except by a fresh one, which wants what is true from
   * the time it was asked for
   */
  load(username: string, { now = Date.now(), fresh = false } = {}): Promise<void> {
    const entry = this.#entries.get(username);
    if (!fresh && entry && now - entry.at < SUMMARY_TTL_MS) return Promise.resolve();
    const running = this.#loading.get(username);
    if (running && !fresh) return running;
    const turn = ++this.#turn;
    const loading = call(() =>
      api.GET("/api/v0/users/{username}/summary", { params: byUser(username) })
    )
      .then((outcome) => {
        // a summary that can't be fetched leaves the card with what it had; the link still works
        if (outcome.ok) this.#put(outcome.data, turn);
      })
      .finally(() => {
        if (this.#loading.get(username) === loading) this.#loading.delete(username);
      });
    this.#loading.set(username, loading);
    return loading;
  }

  /** Every user the directory lists for `query`, read a page at a time; each summary is kept */
  every = async (query: DirectoryQuery): Promise<Outcome<UserSummary[]>> => {
    const found: UserSummary[] = [];
    for (let offset = 0; ;) {
      const turn = ++this.#turn;
      const outcome = await call(() =>
        api.GET("/api/v0/users", { params: { query: { ...query, offset, limit: 500 } } })
      );
      if (!outcome.ok) return outcome;
      for (const summary of outcome.data.users) this.#put(summary, turn);
      found.push(...outcome.data.users);
      offset += outcome.data.users.length;
      if (!outcome.data.users.length || offset >= outcome.data.total) {
        return { ok: true, data: found };
      }
    }
  };

  /** Reads the buddy list into `buddies` unless it was read less than a minute ago */
  loadBuddies(now = Date.now()): Promise<void> {
    if (now - this.#buddiesAt < SUMMARY_TTL_MS) return Promise.resolve();
    // read by name, so a buddy changed between pages is not missed
    this.#buddiesLoading ??= this.every({ buddy: true, sort: "username", order: "asc" })
      .then((outcome) => {
        // a list that can't be read leaves the names as they were, to be read on the next call
        if (!outcome.ok) return;
        const found = new Set(outcome.data.map((u) => u.username));
        for (const name of this.buddies) if (!found.has(name)) this.buddies.delete(name);
        this.#buddiesAt = now;
      })
      .finally(() => (this.#buddiesLoading = null));
    return this.#buddiesLoading;
  }

  /** The usernames blocked now; null when the directory couldn't be read */
  blocked = async (): Promise<Set<string> | null> => {
    const outcome = await this.every({ blocked: true });
    return outcome.ok ? new Set(outcome.data.map((u) => u.username)) : null;
  };

  clear() {
    this.#entries.clear();
    this.buddies.clear();
    this.#buddiesAt = -Infinity;
    this.#loading.clear();
    this.#taken.clear();
    this.#cleared = this.#turn;
  }

  /** Takes `summary`, read or told with number `turn`, unless something newer was taken */
  #put(summary: UserSummary, turn: number) {
    if (turn <= this.#cleared || turn < (this.#taken.get(summary.username) ?? 0)) return;
    this.#taken.set(summary.username, turn);
    if (summary.buddy) this.buddies.add(summary.username);
    else this.buddies.delete(summary.username);
    const entry = this.#entries.get(summary.username);
    if (entry) {
      patch(entry.summary, summary);
      entry.at = Date.now();
    } else {
      const fresh = $state({ summary, at: Date.now() });
      this.#entries.set(summary.username, fresh);
    }
  }
}

export const users = new Users();
