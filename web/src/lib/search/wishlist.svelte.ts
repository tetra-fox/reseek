import { api, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

export type Wish = Schemas["WishlistItem"];

/**
 * The wishlist: saved searches the daemon runs again one at a time on the server's interval. A
 * turn that found new files is read again from its frame
 */
class Wishlist {
  rows = new Keyed<Wish>((w) => w.id);
  loaded = $state(false);
  error = $state<string | null>(null);

  constructor() {
    live.on("search.wishlist_found", () => void this.load());
  }

  async load() {
    const turn = this.rows.reading();
    const outcome = await call(() => api.GET("/api/v0/searches/wishlist"));
    // a later read, or the end of the session, came meanwhile
    if (!this.rows.latest(turn)) return;
    if (outcome.ok) {
      this.rows.reconcile(outcome.data, turn);
      this.error = null;
    } else {
      this.rows.abandon(turn);
      this.error = outcome.error;
    }
    this.loaded = true;
  }

  /** Forgets the wishlist, as the session ends */
  clear() {
    this.rows.clear();
    this.loaded = false;
    this.error = null;
  }

  async add(searchText: string) {
    const outcome = await call(() =>
      api.POST("/api/v0/searches/wishlist", { body: { searchText } })
    );
    if (outcome.ok) this.rows.upsert(outcome.data);
    return outcome;
  }

  async remove(id: string) {
    const outcome = await call(() =>
      api.DELETE("/api/v0/searches/wishlist/{id}", { params: { path: { id } } })
    );
    if (outcome.ok) this.rows.remove(id);
    return outcome;
  }
}

export const wishlist = new Wishlist();
