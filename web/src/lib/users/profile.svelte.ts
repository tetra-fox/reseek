import { api, type Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { Loader } from "#lib/loader.svelte.js";
import { call } from "#lib/request.js";

export type OwnProfile = Schemas["OwnProfile"];

/**
 * Our own profile as peers are sent it, fetched on first need and again whenever the options
 * change, since a profile write is an option write
 */
class Own {
  readonly loader = new Loader(() => call(() => api.GET("/api/v0/profile")));
  #asked = false;

  constructor() {
    live.on("application.options", () => {
      if (this.#asked) void this.loader.load();
    });
  }

  get data(): OwnProfile | undefined {
    return this.loader.data;
  }

  /** Fetches the profile unless it was fetched before */
  need() {
    if (this.#asked) return;
    this.#asked = true;
    void this.loader.load();
  }
}

export const ownProfile = new Own();

/** The first `n` lines of a text, as the hover card shows a profile */
export const firstLines = (text: string, n: number) => text.split("\n").slice(0, n).join("\n");
