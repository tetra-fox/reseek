import { api, type Schemas } from "#lib/api/client.js";
import { type Editability, editability } from "#lib/config/editability.js";
import type { ConfigView } from "#lib/config/fields.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { call } from "#lib/request.js";

import { ownProfile } from "./profile.svelte";

export type Interests = Schemas["Interests"];

/** What the interests editor's card says about the lists */
export const INTERESTS_HINT = "Used to suggest similar users";

/** Which of the two lists */
export type Side = keyof Interests;

/** The options that hold the lists */
const KEYS: Record<Side, string> = {
  liked: "soulseek.interests.liked",
  hated: "soulseek.interests.hated"
};

const RANK: Record<Editability["mode"], number> = { save: 0, runtime: 1, locked: 2 };

/** How the lists can be changed from a page: as the option of the two that allows the least */
export const interestsEditability = (view: ConfigView): Editability => {
  const [liked, hated] = [
    editability(view, KEYS.liked, "page"),
    editability(view, KEYS.hated, "page")
  ];
  return RANK[hated.mode] > RANK[liked.mode] ? hated : liked;
};

/** `lists` with `interest` on `side` and off the other list, since it can only be on one */
export const withInterest = (lists: Interests, side: Side, interest: string): Interests => {
  const without = (list: string[]) => list.filter((i) => i !== interest);
  return {
    liked: side === "liked" ? [...without(lists.liked), interest] : without(lists.liked),
    hated: side === "hated" ? [...without(lists.hated), interest] : without(lists.hated)
  };
};

/**
 * What we like and dislike, the lists of our own profile. A change shows at once and is saved as
 * it is made; each save replaces both lists, so they go out one at a time, in the order they
 * were made, and the last one decides what the configuration holds. Once none is left to go
 * out, the profile is read again and the lists are its own
 */
export class OwnInterests {
  /** the lists since a change, ahead of the profile */
  #changed = $state.raw<Interests | null>(null);
  #saves: Promise<void> = Promise.resolve();
  /** saves made and not yet answered */
  #pending = 0;

  constructor() {
    ownProfile.need();
  }

  get lists(): Interests {
    return this.#changed ?? ownProfile.data?.interests ?? { liked: [], hated: [] };
  }

  /** Resolves once every change made so far has been answered */
  get settled(): Promise<void> {
    return this.#saves;
  }

  add = (side: Side, text: string) => {
    // in the form the server is told, so an interest moved from the other list leaves it
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
      const outcome = await call(() => api.PUT("/api/v0/profile/interests", { body: lists }));
      this.#pending -= 1;
      if (!outcome.ok) toast(outcome.error, "error");
      if (this.#pending > 0) return;
      // after a failed last save, the profile holds what an earlier one saved, or what it had
      await ownProfile.loader.load();
      // a change made during that read is still on its way
      if (this.#pending === 0) this.#changed = null;
    });
  }
}
