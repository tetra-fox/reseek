import { goto } from "$app/navigation";
import { onDestroy } from "svelte";

import { api } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { SERVER } from "#lib/chat/notices.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { userHref } from "#lib/nav.js";
import { call } from "#lib/request.js";
import { TYPING_PAUSE } from "#lib/ui/keys.js";

import { profileOf } from "./actions";
import { bestMatches, CANDIDATES } from "./match";
import type { UserSummary } from "./users.svelte";

const LISTED = 8;

/** The entry that opens a user, or looks them up when the directory does not hold them */
export const userValue = (name: string) => `user:${name}`;

/** The name in an entry from `userValue` */
export const userOf = (value: string) => value.slice(userValue("").length);

/**
 * The users a text finds for a search box: those the directory holds and, for a name it does
 * not, a look-up that asks the network with the name as typed. Made while a component starts,
 * and follows the text for as long as the component lives
 */
export class UserFinder {
  /** the users the directory matched for the text; tags and notes match too */
  found = $state.raw<UserSummary[]>([]);
  /** the name being asked of the network */
  #looking = $state<string | null>(null);
  #text: () => string;
  #tab: string | undefined;
  #onmissed: (() => void) | undefined;
  /** the text the directory last answered for, and the read under way or done */
  #answered = "";
  #reading: { text: string; done: Promise<void> } | null = null;

  /** the closest of the users found */
  readonly matches = $derived.by(() =>
    bestMatches(this.found, this.#text(), (u) => u.username).slice(0, LISTED)
  );
  /** the text can be a name the directory does not hold; `server` is never a user */
  readonly unknown = $derived.by(() => {
    const text = this.#text();
    return !!text && text !== SERVER && !this.found.some((u) => u.username === text);
  });
  /** the network is being asked about the text */
  readonly asking = $derived.by(() => this.#looking !== null && this.#looking === this.#text());
  /** the entry the list starts with that can be taken, null while none can */
  readonly first = $derived.by(() => {
    if (this.matches.length) return userValue(this.matches[0].username);
    const open = this.unknown && app.isServerConnected && !this.asking;
    return open ? userValue(this.#text()) : null;
  });

  /**
   * `text` gives the box's text, trimmed. `tab` is the tab of the profile a pick opens, `onread`
   * hears the text once typing stops, and `onmissed` when a look-up found nobody
   */
  constructor(
    text: () => string,
    options: { tab?: string; onread?: (text: string) => void; onmissed?: () => void } = {}
  ) {
    this.#text = text;
    this.#tab = options.tab;
    this.#onmissed = options.onmissed;
    $effect(() => {
      const value = text();
      const timer = setTimeout(() => {
        options.onread?.(value);
        void this.#read();
      }, TYPING_PAUSE);
      return () => clearTimeout(timer);
    });
    onDestroy(this.abandon);
  }

  #read = () => {
    const text = this.#text();
    if (this.#reading?.text !== text) this.#reading = { text, done: this.#ask(text) };
    return this.#reading.done;
  };

  async #ask(text: string) {
    const outcome = text
      ? await call(() =>
          api.GET("/api/v0/users", { params: { query: { q: text, limit: CANDIDATES } } })
        )
      : null;
    if (this.#text() !== text) return;
    this.#answered = text;
    // a directory that can't be read leaves the users already listed
    if (!outcome) this.found = [];
    else if (outcome.ok) this.found = outcome.data.users;
  }

  /** Resolves once the directory has answered for the text as it is now */
  settled = () => (this.#answered === this.#text() ? undefined : this.#read());

  /** Forgets the look-up running, whose answer then opens nothing */
  abandon = () => {
    this.#looking = null;
  };

  async #lookUp(name: string) {
    this.#looking = name;
    const outcome = await profileOf(name, true);
    if (this.#looking !== name) return;
    this.#looking = null;
    if (this.#text() !== name) return;
    if (outcome.ok) return void goto(userHref(name, this.#tab));
    this.#onmissed?.();
    toast(outcome.status === 404 ? `No user named ${name}` : outcome.error, "error");
  }

  /** Opens the user an entry names, one the directory does not hold once the network knows them */
  choose = (value: string) => {
    const name = userOf(value);
    if (this.found.some((u) => u.username === name)) void goto(userHref(name, this.#tab));
    else void this.#lookUp(name);
  };
}
