import { SvelteMap } from "svelte/reactivity";

import { api } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import type { Root } from "#lib/localfiles.js";
import { call } from "#lib/request.js";
import { fields } from "#lib/stored.svelte.js";

import type { Report } from "./analysis";
import { type Mark, markOf } from "./marks";

/** How many reports one read of the list asks for */
const PAGE = 500;

/**
 * The mark of every analyzed file, for lists to show a chip and filter by, read once while a list
 * that shows them is open and then only what was analyzed since
 */
class Marks {
  #files = new SvelteMap<string, Mark>();
  #downloads = new SvelteMap<string, Mark>();
  /** when the newest report read so far finished, which the next read starts after */
  #newest: string | null = null;
  #holders = 0;
  #stops: (() => void)[] = [];
  #reading = false;
  /** a report finished while a read ran, which that read may have missed */
  #again = false;

  /** Keeps the marks current while the returned function has not been called */
  retain(): () => void {
    if (this.#holders++ === 0) {
      this.#stops = [live.retain("transfers"), live.on("files.analysis", () => void this.#read())];
      void this.#read();
    }
    return () => {
      if (--this.#holders > 0) return;
      for (const stop of this.#stops) stop();
      this.#stops = [];
    };
  }

  /** Takes in a report read on its own, such as one whose judgment just changed */
  put(report: Report) {
    const mark = markOf(report);
    this.#files.set(`${report.root}:${report.relativePath}`, mark);
    if (report.transferId) this.#downloads.set(report.transferId, mark);
    // compared as times: the fractions of a second can differ in length
    if (!this.#newest || Date.parse(report.analyzedAt) > Date.parse(this.#newest)) {
      this.#newest = report.analyzedAt;
    }
  }

  /** The mark of a file under a root, if it has been analyzed */
  file(root: Root, path: string): Mark | undefined {
    return this.#files.get(`${root}:${path}`);
  }

  /** The mark of a download's file, if it has been analyzed */
  download(id: string): Mark | undefined {
    return this.#downloads.get(id);
  }

  /** Reads every report finished since the newest one read, once more if one finishes meanwhile */
  async #read() {
    if (this.#reading) {
      this.#again = true;
      return;
    }
    this.#reading = true;
    do {
      this.#again = false;
      await this.#pages();
    } while (this.#again);
    this.#reading = false;
  }

  async #pages() {
    const since = this.#newest ?? undefined;
    for (let offset = 0; ; offset += PAGE) {
      const outcome = await call(() =>
        api.GET("/api/v0/analysis", { params: { query: { since, offset, limit: PAGE } } })
      );
      // a failed read leaves the marks as they are; the next report finished reads again
      if (!outcome.ok) return;
      for (const report of outcome.data) this.put(report);
      if (outcome.data.length < PAGE) return;
    }
  }
}

export const marks = new Marks();

/** The extensions the analyzer is offered for, lowercased, as the options name them */
const extensions = $derived.by(() => {
  const listed = fields(fields(app.options).analysis).extensions;
  return new Set(Array.isArray(listed) ? listed.map((x) => String(x).toLowerCase()) : []);
});

/** Whether the analyzer is offered for any kind of file */
export const analyzerOffered = () => extensions.size > 0;

/** Whether the analyzer is offered for a file, by its name's extension */
export const analyzable = (name: string) =>
  extensions.has(name.split(".").pop()?.toLowerCase() ?? "");
