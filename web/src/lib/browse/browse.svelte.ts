import { SvelteSet } from "svelte/reactivity";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";
import { Expansion, type ExpansionSnapshot } from "#lib/tree/expansion.svelte.js";
import {
  detectSeparator,
  type Listing,
  listingTree,
  type RemoteDirectory,
  type Separator
} from "#lib/tree/folders.js";

import { type BrowseInfo, readListing, writeListing } from "./cache";
import { rememberBrowsed } from "./recent.svelte";

export type BrowsePhase = "idle" | "pending" | "complete" | "error";

/** The most files one selection may hold, which keeps the saved selection inside storage limits */
export const MAX_SELECTED_FILES = 5000;

const META_KEY = "reseek-browse-state";

/** What is kept of the tab besides the listing: never a browse under way, which can't resume */
type Meta = {
  username: string;
  phase: "idle" | "complete" | "error";
  errorMessage: string;
  separator: Separator;
  info: BrowseInfo | null;
  /** the open folders */
  expansion: ExpansionSnapshot | null;
  /** full paths of the ticked files */
  selected: string[];
};

const EMPTY_TREE: Listing = { roots: [], byPath: new Map() };

/**
 * What the Shares tab shows: one user's shares at a time, fetched whole, kept as a tree with the
 * folders open and the files ticked in it, and remembered in this browser until another user is
 * browsed. Progress comes from the `users` topic while a browse arrives, or from polling its
 * status while the stream is down
 */
class Browse {
  username = $state("");
  phase = $state<BrowsePhase>("idle");
  errorMessage = $state("");
  progress = $state.raw<Schemas["BrowseProgress"] | null>(null);
  tree = $state.raw<Listing>(EMPTY_TREE);
  separator = $state<Separator>("\\");
  info = $state.raw<BrowseInfo | null>(null);
  /** which folders are open */
  expansion = new Expansion(false);
  /** full paths of the ticked files */
  selected = new SvelteSet<string>();
  /** a folder the tab goes to once it shows the listing, then forgets */
  target = $state<string | null>(null);
  /** a saved listing is being read back */
  restoring = $state(false);
  #restored = false;

  constructor() {
    live.on("users.browse_progress", (progress) => {
      if (this.phase === "pending" && progress.username === this.username) {
        this.progress = progress;
      }
    });
  }

  /** Asks `name` for everything they share */
  browse = async (name: string) => {
    // a name is used as written, spaces at its ends too, which make it another user's
    if (!name.trim()) return;
    // a browse of someone else under way is left to finish unseen: its answer is dropped
    if (this.phase === "pending" && this.username === name) return;
    this.username = name;
    this.phase = "pending";
    this.errorMessage = "";
    this.progress = null;
    this.selected.clear();
    this.target = null;
    const outcome = await call(() =>
      api.GET("/api/v0/users/{username}/browse", { params: byUser(name) })
    );
    if (this.username !== name || this.phase !== "pending") return;
    if (!outcome.ok) {
      this.phase = "error";
      this.errorMessage = outcome.error;
      this.save();
      return;
    }
    const { directories, lockedDirectories } = outcome.data;
    const all: RemoteDirectory[] = [
      ...directories,
      ...lockedDirectories.map((d) => ({ ...d, locked: true }))
    ];
    const count = (list: Schemas["Directory"][]) => list.reduce((n, d) => n + d.files.length, 0);
    const info: BrowseInfo = {
      directories: directories.length,
      files: count(directories),
      lockedDirectories: lockedDirectories.length,
      lockedFiles: count(lockedDirectories),
      bytes: all.reduce((n, d) => n + d.files.reduce((m, f) => m + f.size, 0), 0)
    };
    this.#show(name, all, detectSeparator(all), info);
    // a new listing starts with its first folder open, a hint that folders open
    this.expansion.reset(false);
    const first = this.tree.roots[0];
    if (first?.children.length) this.expansion.set(first.path, true);
    this.phase = "complete";
    rememberBrowsed(name);
    this.save();
    const listing = { username: name, directories: all, separator: this.separator, info };
    // a listing that can't be kept is browsed again after a reload
    setTimeout(() => void writeListing(listing).catch(() => {}));
  };

  /** Asks how far the browse got, for while the stream is down; no answer yet means no progress */
  poll = async () => {
    if (this.phase !== "pending") return;
    const name = this.username;
    const outcome = await call(() =>
      api.GET("/api/v0/users/{username}/browse/status", { params: byUser(name) })
    );
    if (outcome.ok && this.phase === "pending" && this.username === name) {
      this.progress = outcome.data;
    }
  };

  /** Back to nothing shown, the kept browse forgotten */
  clear = () => {
    this.username = "";
    this.phase = "idle";
    this.errorMessage = "";
    this.progress = null;
    this.tree = EMPTY_TREE;
    this.separator = "\\";
    this.info = null;
    this.expansion.reset(false);
    this.selected.clear();
    this.target = null;
    this.save();
    // nothing to read back when the store is unavailable
    void writeListing(null).catch(() => {});
  };

  /** Ticks or unticks files; false when that would pass the limit */
  tick = (filenames: string[], on: boolean): boolean => {
    if (on) {
      const added = filenames.filter((f) => !this.selected.has(f)).length;
      if (this.selected.size + added > MAX_SELECTED_FILES) {
        toast("At most 5,000 files can be selected", "error");
        return false;
      }
    }
    for (const f of filenames) {
      if (on) this.selected.add(f);
      else this.selected.delete(f);
    }
    return true;
  };

  /**
   * Brings back the browse of `username` as it was left, when that is what is kept, once per page
   * load
   */
  restore = async (username: string) => {
    if (this.#restored) return;
    this.#restored = true;
    let meta: Meta | null = null;
    try {
      meta = readMeta(JSON.parse(localStorage.getItem(META_KEY) ?? "null"));
    } catch {
      // nothing readable saved: the user is browsed anew
    }
    // a browse under way, as one a link started, wins over what was saved
    if (meta?.username !== username || this.phase !== "idle") return;
    this.username = meta.username;
    this.restoring = true;
    // an IndexedDB that can't be read (blocked, private window) leaves only the saved meta
    const listing = await readListing().catch(() => undefined);
    this.restoring = false;
    // a browse started while the listing was read wins too
    if (this.phase !== "idle") return;
    this.phase = meta.phase;
    this.errorMessage = meta.errorMessage;
    if (listing) {
      this.#show(listing.username, listing.directories, listing.separator, listing.info);
      if (meta.expansion) this.expansion.restore(meta.expansion);
      for (const f of meta.selected) this.selected.add(f);
    } else {
      this.separator = meta.separator;
      this.info = meta.info;
      if (meta.phase === "complete") this.phase = "idle";
    }
    this.save();
  };

  /** Keeps the browse as it is in this browser: the user, the open folders, the ticked files */
  save = () => {
    const meta: Meta = {
      username: this.username,
      phase: this.phase === "pending" ? "idle" : this.phase,
      errorMessage: this.errorMessage,
      separator: this.separator,
      info: this.info,
      expansion: this.expansion.snapshot(),
      selected: [...this.selected]
    };
    try {
      localStorage.setItem(META_KEY, JSON.stringify(meta));
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        toast("The selection is too large to save", "error");
      }
      // other refusals (blocked storage) leave the screen as it is, unsaved
    }
  };

  #show(username: string, directories: RemoteDirectory[], separator: Separator, info: BrowseInfo) {
    this.username = username;
    this.separator = separator;
    this.info = info;
    this.tree = listingTree(directories, separator);
  }
}

const strings = (list: unknown) =>
  Array.isArray(list) ? list.filter((f): f is string => typeof f === "string") : [];

/** A saved meta as far as it makes sense; a browse saved as under way comes back as idle */
const readMeta = (saved: unknown): Meta | null => {
  const s = fields(saved);
  if (typeof s.username !== "string") return null;
  const phase = s.phase === "complete" || s.phase === "error" ? s.phase : "idle";
  const expansion = fields(s.expansion);
  return {
    username: s.username,
    phase,
    errorMessage: typeof s.errorMessage === "string" ? s.errorMessage : "",
    separator: s.separator === "/" ? "/" : "\\",
    info: s.info ? (s.info as BrowseInfo) : null,
    expansion:
      typeof expansion.openByDefault === "boolean"
        ? { openByDefault: expansion.openByDefault, flipped: strings(expansion.flipped) }
        : null,
    selected: strings(s.selected)
  };
};

export const browser = new Browse();
