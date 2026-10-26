import { SvelteMap } from "svelte/reactivity";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import { patch } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

export type UserSummary = Schemas["UserSummary"];

/** How long a fetched summary is used before it is fetched again */
export const SUMMARY_TTL_MS = 60_000;

type Entry = { summary: UserSummary; at: number };

/**
 * What the directory says about users, by username, for the username menu and hover card:
 * fetched on first need from stored data only (never a Soulseek round trip), kept a minute, and
 * patched by `users.updated` frames while a screen follows the `users` topic
 */
class Users {
  #entries = new SvelteMap<string, Entry>();
  #loading = new Map<string, Promise<void>>();

  constructor() {
    live.on("users.updated", (summary) => this.#put(summary));
    live.on("users.removed", ({ username }) => this.#entries.delete(username));
  }

  /** The summary held for `username`, whatever its age */
  get(username: string): UserSummary | undefined {
    return this.#entries.get(username)?.summary;
  }

  /** Fetches the summary unless one younger than a minute is held */
  load(username: string, now = Date.now()): Promise<void> {
    const entry = this.#entries.get(username);
    if (entry && now - entry.at < SUMMARY_TTL_MS) return Promise.resolve();
    const running = this.#loading.get(username);
    if (running) return running;
    const loading = call(() =>
      api.GET("/api/v0/users/{username}/summary", { params: byUser(username) })
    )
      .then((outcome) => {
        // a summary that can't be fetched leaves the card with what it had; the link still works
        if (outcome.ok) this.#put(outcome.data);
      })
      .finally(() => this.#loading.delete(username));
    this.#loading.set(username, loading);
    return loading;
  }

  clear() {
    this.#entries.clear();
  }

  #put(summary: UserSummary) {
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
