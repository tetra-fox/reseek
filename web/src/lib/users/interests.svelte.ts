import { api, type Schemas } from "#lib/api/client.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { Loader } from "#lib/loader.svelte.js";
import { call } from "#lib/request.js";

export type Interests = Schemas["Interests"];

/** Which of the two lists */
export type Side = keyof Interests;

/** `lists` with `interest` on `side` and off the other list, since it can only be on one */
export const withInterest = (lists: Interests, side: Side, interest: string): Interests => {
  const without = (list: string[]) => list.filter((i) => i !== interest);
  return {
    liked: side === "liked" ? [...without(lists.liked), interest] : without(lists.liked),
    hated: side === "hated" ? [...without(lists.hated), interest] : without(lists.hated)
  };
};

/**
 * What we like and dislike. A change shows at once and is saved as it is made; each save replaces
 * both lists, so they go out one at a time, in the order they were made, and the last one decides
 * what the daemon holds. Once none is left to go out, the lists are the daemon's again
 */
export class OwnInterests {
  readonly loader = new Loader(() => call(() => api.GET("/api/v0/interests")));
  /** the lists since a change, ahead of what the daemon has answered */
  #changed = $state.raw<Interests | null>(null);
  #saves: Promise<void> = Promise.resolve();
  /** saves made and not yet answered */
  #pending = 0;

  get lists(): Interests {
    return this.#changed ?? this.loader.data ?? { liked: [], hated: [] };
  }

  /** Resolves once every change made so far has been answered */
  get settled(): Promise<void> {
    return this.#saves;
  }

  add = (side: Side, text: string) => {
    // in the form the daemon keeps, so an interest moved from the other list leaves it
    const interest = text.trim().toLowerCase();
    if (interest && !this.lists[side].includes(interest)) {
      this.#save(withInterest(this.lists, side, interest));
    }
  };

  remove = (side: Side, interest: string) => {
    const lists = this.lists;
    this.#save({ ...lists, [side]: lists[side].filter((i) => i !== interest) });
  };

  #save(lists: Interests) {
    this.#changed = lists;
    this.#pending += 1;
    this.#saves = this.#saves.then(async () => {
      const outcome = await call(() => api.PUT("/api/v0/interests", { body: lists }));
      this.#pending -= 1;
      if (outcome.ok) this.loader.data = outcome.data;
      else toast(outcome.error, "error");
      if (this.#pending > 0) return;
      // a failed last save leaves the daemon with what an earlier one saved, or what it had
      if (!outcome.ok) await this.loader.load();
      // a change made during that read is still on its way
      if (this.#pending === 0) this.#changed = null;
    });
  }
}
