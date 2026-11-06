import { goto } from "$app/navigation";
import { navigating, page } from "$app/state";
import { untrack } from "svelte";

import { app } from "#lib/app.svelte.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { formatBytes } from "#lib/format.js";
import { href, route } from "#lib/nav.js";

import { analysisHref, parseTarget, type SlotId } from "./address";
import { type Target, targetKey } from "./analysis";
import { analyzable } from "./marks.svelte";
import { type Sending, sendFile } from "./upload";

/** A file in a slot; its name is null until something says it, as for a file named by a link */
export type Loaded = { target: Target; name: string | null };

/** A screen's address with its query written one way, so two spellings of it compare equal */
const addressOf = (url: { hash: string }) => {
  const { path, query } = route(url);
  return `${path}?${query}`;
};

/** Whether a target is a local file, uploaded for this one look */
export const isLocal = (t: Target) => t.kind === "file" && t.root === "temporary";

/**
 * What the Analysis page's two slots hold in this tab. The address is what says so for a file on
 * the daemon: this follows it, and a change here goes to it. A local file cannot be in a link, so
 * it is held here alone and is gone with a reload. Nothing is kept past the tab, so a file left
 * in a slot long ago never counts as loaded
 */
class Slots {
  a = $state.raw<Loaded | null>(null);
  b = $state.raw<Loaded | null>(null);
  /** the files on their way to the daemon, by the slot each will fill */
  sending = $state.raw<Partial<Record<SlotId, Sending>>>({});

  /** whether Compare can be offered, as it fills the second slot beside something in the first */
  get occupied() {
    return !!this.a || !!this.sending.a;
  }

  /** The link to the page as it stands */
  get href() {
    return analysisHref(this.a?.target ?? null, this.b?.target ?? null);
  }

  /** Whether the slot holds a local file */
  local(id: SlotId) {
    return !!this[id] && isLocal(this[id].target);
  }

  /**
   * Takes what the address says: a slot it leaves out empties, unless a local file is there. The
   * slots are read untracked, so an effect that follows the address runs again only when the
   * address changes, and a change here is not undone before its own address lands
   */
  follow(query: URLSearchParams) {
    for (const id of ["a", "b"] as const) {
      const named = parseTarget(query.get(id));
      const held = untrack(() => this[id]);
      if (!named && held && isLocal(held.target)) continue;
      if (!named) this[id] = null;
      else if (!held || targetKey(held.target) !== targetKey(named)) {
        this[id] = { target: named, name: null };
      }
    }
  }

  /** Puts a file in a slot and shows the page */
  fill(id: SlotId, target: Target, name: string | null) {
    this.#stopSending(id);
    this[id] = { target, name };
    this.#go();
  }

  /** Fills the first slot from a list */
  open = (target: Target, name: string) => this.fill("a", target, name);

  /** Fills the second slot from a list */
  compare = (target: Target, name: string) => this.fill("b", target, name);

  clear(id: SlotId) {
    this.#stopSending(id);
    this[id] = null;
    this.#go();
  }

  swap() {
    [this.a, this.b] = [this.b, this.a];
    this.#go();
  }

  /**
   * Shows the page as the slots stand, moving the address only when it changes so Back leaves in
   * one press. A move still on its way counts as the address, and returning to the address shown
   * takes that move's place
   */
  #go() {
    const next = addressOf({ hash: this.href });
    const shown = addressOf(page.url);
    if (next === addressOf(navigating.to?.url ?? page.url)) return;
    void goto(this.href, {
      // on the page itself scroll and focus stay where they are
      reset: route(page.url).path !== "/analysis",
      replace: next === shown
    });
  }

  /** Sends a local file to the daemon, into the slot once it is there */
  upload(id: SlotId, file: File) {
    if (!analyzable(file.name)) {
      return void toast(`${file.name} is not a type the analyzer reads`, "error");
    }
    // the daemon refuses a larger file as well, but only once it is being sent, which a browser
    // can report as a lost connection rather than as the answer
    const limit = app.state?.process.uploadMaxBytes;
    if (limit !== undefined && file.size > limit) {
      const largest = formatBytes(limit);
      return void toast(`${file.name} is over the ${largest} upload limit`, "error");
    }
    this.#stopSending(id);
    if (this[id]) this.clear(id);
    const mark = (sent: number) => {
      const held = this.sending[id];
      if (held) this.sending = { ...this.sending, [id]: { ...held, sent } };
    };
    const cancel = sendFile(file, mark, (outcome) => {
      this.#drop(id);
      // a local file is never in the address, so it fills the slot and leaves the user where
      // they are
      if (outcome.ok) {
        const { root, path } = outcome.data;
        this[id] = { target: { kind: "file", root, path }, name: file.name };
      } else {
        toast(`Could not upload ${file.name}: ${outcome.error}`, "error");
      }
    });
    this.sending = { ...this.sending, [id]: { name: file.name, sent: 0, cancel } };
  }

  /** Stops the file on its way to a slot */
  #stopSending(id: SlotId) {
    this.sending[id]?.cancel();
    this.#drop(id);
  }

  #drop(id: SlotId) {
    const { [id]: gone, ...rest } = this.sending;
    if (gone) this.sending = rest;
  }

  /** Stops the file on its way to a slot, which stays empty */
  cancel(id: SlotId) {
    this.#stopSending(id);
  }
}

export const slots = new Slots();

/** The link to a screen of the navigation: the Analysis page comes back as it was left */
export const screenHref = (path: string) => (path === "/analysis" ? slots.href : href(path));
