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
 * both lists, so they go out one at a time, in the order they were made
 */
export class OwnInterests {
  readonly loader = new Loader(() => call(() => api.GET("/api/v0/interests")));
  /** the lists since a change, ahead of what the daemon has answered */
  #changed = $state.raw<Interests | null>(null);
  #saves: Promise<void> = Promise.resolve();

  get lists(): Interests {
    return this.#changed ?? this.loader.data ?? { liked: [], hated: [] };
  }

  add = (side: Side, text: string) => {
    const interest = text.trim();
    if (interest) this.#save(withInterest(this.lists, side, interest));
  };

  remove = (side: Side, interest: string) => {
    const lists = this.lists;
    this.#save({ ...lists, [side]: lists[side].filter((i) => i !== interest) });
  };

  #save(lists: Interests) {
    this.#changed = lists;
    this.#saves = this.#saves.then(async () => {
      const outcome = await call(() => api.PUT("/api/v0/interests", { body: lists }));
      if (outcome.ok) return;
      toast(outcome.error, "error");
      await this.loader.load();
      this.#changed = null;
    });
  }
}
