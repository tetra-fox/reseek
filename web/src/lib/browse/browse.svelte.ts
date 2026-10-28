import { SvelteSet } from "svelte/reactivity";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";
import { toast } from "#lib/toast.svelte.js";

import { type BrowseInfo, readListing, writeListing } from "./cache";
import { rememberBrowsed } from "./recent.svelte";
import {
  buildTree,
  detectSeparator,
  type RemoteDirectory,
  type Separator,
  type Tree
} from "./tree";

export type BrowsePhase = "idle" | "pending" | "complete" | "error";

/** The most files one selection may hold, which keeps the saved selection inside storage limits */
export const MAX_SELECTED_FILES = 5000;

const META_KEY = "reseek-browse-state";

/** What is kept of the screen besides the listing: never a browse under way, which can't resume */
type Meta = {
  username: string;
  phase: "idle" | "complete" | "error";
  errorMessage: string;
  separator: Separator;
  info: BrowseInfo | null;
  selected: { directoryName: string; files: string[]; subdirectory: string | null } | null;
};

const EMPTY_TREE: Tree = { roots: [], byName: new Map() };

/**
 * The Browse screen's state: one user's shares at a time, fetched whole, kept as a tree, and
 * remembered in this browser until it is cleared. Progress comes from the `users` topic while a
 * browse arrives, or from polling its status while the stream is down
 */
class Browse {
  username = $state("");
  phase = $state<BrowsePhase>("idle");
  errorMessage = $state("");
  progress = $state.raw<Schemas["BrowseProgress"] | null>(null);
  tree = $state.raw<Tree>(EMPTY_TREE);
  separator = $state<Separator>("\\");
  info = $state.raw<BrowseInfo | null>(null);
  /** the folder shown in the selection card */
  directory = $state<string | null>(null);
  /** full filenames of the files ticked in it */
  selected = new SvelteSet<string>();
  /** the one child folder expanded in the selection card */
  subdirectory = $state<string | null>(null);
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

  /** Asks `username` for everything they share */
  browse = async (username: string) => {
    const name = username.trim();
    // a browse of someone else under way is left to finish unseen: its answer is dropped
    if (!name || (this.phase === "pending" && this.username === name)) return;
    this.username = name;
    this.phase = "pending";
    this.errorMessage = "";
    this.progress = null;
    this.#select(null);
    const outcome = await call(() =>
      api.GET("/api/v0/users/{username}/browse", { params: byUser(name) })
    );
    if (this.username !== name || this.phase !== "pending") return;
    if (!outcome.ok) {
      this.phase = "error";
      this.errorMessage = outcome.error;
      this.#saveMeta();
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
    this.phase = "complete";
    rememberBrowsed(name);
    this.#saveMeta();
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

  /** Back to an empty screen, the kept browse forgotten */
  clear = () => {
    this.username = "";
    this.phase = "idle";
    this.errorMessage = "";
    this.progress = null;
    this.tree = EMPTY_TREE;
    this.separator = "\\";
    this.info = null;
    this.#select(null);
    this.#saveMeta();
    // nothing to read back when the store is unavailable
    void writeListing(null).catch(() => {});
  };

  /** Shows a folder in the selection card, its first child expanded, nothing ticked */
  select = (name: string | null) => {
    this.#select(name);
    this.#saveMeta();
  };

  /** Expands one child folder of the selection, or none */
  expand = (name: string | null) => {
    this.subdirectory = name;
    this.#saveMeta();
  };

  /** Ticks or unticks files of the selection; false when that would pass the limit */
  tick = (filenames: string[], on: boolean): boolean => {
    if (on) {
      const added = filenames.filter((f) => !this.selected.has(f)).length;
      if (this.selected.size + added > MAX_SELECTED_FILES) {
        toast("You can select at most 5,000 files.", "error");
        return false;
      }
    }
    for (const f of filenames) {
      if (on) this.selected.add(f);
      else this.selected.delete(f);
    }
    this.#saveMeta();
    return true;
  };

  /** Brings back the screen as it was left, once per page load */
  restore = async () => {
    if (this.#restored) return;
    this.#restored = true;
    let meta: Meta | null = null;
    try {
      meta = readMeta(JSON.parse(localStorage.getItem(META_KEY) ?? "null"));
    } catch {
      // nothing readable saved: the screen starts empty
    }
    // a browse under way, as one a link started, wins over what was saved
    if (!meta?.username || this.phase !== "idle") return;
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
    } else {
      this.separator = meta.separator;
      this.info = meta.info;
      if (meta.phase === "complete") this.phase = "idle";
    }
    const chosen = meta.selected;
    if (chosen && this.tree.byName.has(chosen.directoryName)) {
      this.directory = chosen.directoryName;
      this.subdirectory = chosen.subdirectory;
      for (const f of chosen.files) this.selected.add(f);
    }
    this.#saveMeta();
  };

  #show(username: string, directories: RemoteDirectory[], separator: Separator, info: BrowseInfo) {
    this.username = username;
    this.separator = separator;
    this.info = info;
    this.tree = buildTree(directories, separator);
  }

  #select(name: string | null) {
    this.directory = name;
    this.selected.clear();
    this.subdirectory = name ? (this.tree.byName.get(name)?.children[0]?.name ?? null) : null;
  }

  #saveMeta() {
    const meta: Meta = {
      username: this.username,
      phase: this.phase === "pending" ? "idle" : this.phase,
      errorMessage: this.errorMessage,
      separator: this.separator,
      info: this.info,
      selected: this.directory
        ? {
            directoryName: this.directory,
            files: [...this.selected],
            subdirectory: this.subdirectory
          }
        : null
    };
    try {
      localStorage.setItem(META_KEY, JSON.stringify(meta));
    } catch (e) {
      if (e instanceof DOMException && e.name === "QuotaExceededError") {
        toast("The selection is too large to save. Select fewer files.", "error");
      }
      // other refusals (blocked storage) leave the screen as it is, unsaved
    }
  }
}

/** A saved meta as far as it makes sense; a browse saved as under way comes back as idle */
const readMeta = (saved: unknown): Meta | null => {
  const s = fields(saved);
  if (typeof s.username !== "string") return null;
  const phase = s.phase === "complete" || s.phase === "error" ? s.phase : "idle";
  const selected = fields(s.selected);
  return {
    username: s.username,
    phase,
    errorMessage: typeof s.errorMessage === "string" ? s.errorMessage : "",
    separator: s.separator === "/" ? "/" : "\\",
    info: s.info ? (s.info as BrowseInfo) : null,
    selected:
      typeof selected.directoryName === "string"
        ? {
            directoryName: selected.directoryName,
            files: Array.isArray(selected.files)
              ? selected.files.filter((f): f is string => typeof f === "string")
              : [],
            subdirectory: typeof selected.subdirectory === "string" ? selected.subdirectory : null
          }
        : null
  };
};

export const browser = new Browse();
