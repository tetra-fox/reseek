import { api, byUser, type Schemas } from "#lib/api/client.js";
import { Loader } from "#lib/loader.svelte.js";
import { call, type Outcome } from "#lib/request.js";

export type Recommendations = Schemas["Recommendations"];

/** A user shown beside the recommendations; only users alike to us carry the server's rating */
export type UserLine = { username: string; rating: number | null };

/** Which recommendations are shown: ours, the network's most popular, or those of one interest */
export type Shown = { kind: "personal" } | { kind: "popular" } | { kind: "related"; of: string };

/** Recommendations as read; `popularInstead` when ours came back empty and the popular ones show */
export type Read = { lists: Recommendations; popularInstead: boolean };

const empty = (r: Recommendations) => !r.recommended.length && !r.unrecommended.length;

/**
 * What the server makes of interests: the recommendations shown, and the users beside them, those
 * alike to us or, with an interest picked, those who like it. Ours come back empty until we have
 * interests the server can match, and then the popular ones show in their place
 */
export class InterestFinder {
  shown = $state.raw<Shown>({ kind: "personal" });
  readonly recommendations = new Loader<Read>(() => this.#recommendations());
  readonly users = new Loader<UserLine[]>(() => this.#users());

  /** the interest whose users and recommendations show, null for our own */
  get interest(): string | null {
    return this.shown.kind === "related" ? this.shown.of : null;
  }

  /** Shows `shown` and reads what goes with it */
  show = (shown: Shown) => {
    this.shown = shown;
    void this.recommendations.load();
    void this.users.load();
  };

  async #recommendations(): Promise<Outcome<Read>> {
    const shown = this.shown;
    const read = (outcome: Outcome<Recommendations>, popularInstead = false): Outcome<Read> =>
      outcome.ok ? { ok: true, data: { lists: outcome.data, popularInstead } } : outcome;
    const popular = () => call(() => api.GET("/api/v0/interests/popular"));
    if (shown.kind === "related") {
      return read(
        await call(() =>
          api.GET("/api/v0/interests/{interest}/recommendations", {
            params: { path: { interest: shown.of } }
          })
        )
      );
    }
    if (shown.kind === "popular") return read(await popular());
    const ours = await call(() => api.GET("/api/v0/interests/recommendations"));
    return ours.ok && empty(ours.data) ? read(await popular(), true) : read(ours);
  }

  async #users(): Promise<Outcome<UserLine[]>> {
    const of = this.interest;
    if (of === null) return call(() => api.GET("/api/v0/interests/similar-users"));
    const outcome = await call(() =>
      api.GET("/api/v0/interests/{interest}/users", { params: { path: { interest: of } } })
    );
    if (!outcome.ok) return outcome;
    return { ok: true, data: outcome.data.map((username) => ({ username, rating: null })) };
  }
}

/** What `username` likes and hates, asked of the server */
export const interestsOf = (username: string) =>
  call(() => api.GET("/api/v0/users/{username}/interests", { params: byUser(username) }));
