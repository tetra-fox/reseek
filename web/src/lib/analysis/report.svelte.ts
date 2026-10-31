import { untrack } from "svelte";

import { toast } from "#lib/feedback/toast.svelte.js";
import { inspector, SETTLE_MS } from "#lib/inspector/inspector.svelte.js";
import { session } from "#lib/session.svelte.js";

import { loadReport, queue, type Report, type Target, targetKey } from "./analysis";
import { jobs } from "./jobs.svelte";
import { marks } from "./marks.svelte";
import { Scope } from "./spectrogram/scope.svelte";

/**
 * The analysis of the file the inspector shows: its report, what the viewer holds of it, its job
 * in the queue and whether the large view is open. `target` is the file, null for one the analyzer
 * doesn't take. Make it while a component is being set up
 */
export class FileReport {
  /** undefined while loading, null when the file has no report */
  report = $state.raw<Report | null | undefined>(undefined);
  error = $state("");
  /** what the viewer shows of the report, kept while the report is the same */
  scope = $state.raw<Scope | null>(null);
  /** the viewer across the whole screen */
  large = $state(false);

  #target: () => Target | null;
  #key = $derived.by(() => {
    const target = this.#target();
    return target ? targetKey(target) : null;
  });
  /** the last read asked for, so an answer to an earlier one is dropped */
  #asked = 0;
  /** the file to open large as soon as its spectrogram can show */
  #openLarge = $state<string | null>(null);
  /** whether there is a file, which holds the queue, however often the file changes */
  #any = $derived(this.#key !== null);

  /** the file's job, if it waits or runs in the queue */
  readonly job = $derived.by(() => {
    const target = this.#target();
    return target ? jobs.of(target) : undefined;
  });
  /** whether the file has no report it can show, which an older version's is as good as */
  readonly unread = $derived(this.report === null || this.report?.stale === true);

  constructor(target: () => Target | null) {
    this.#target = target;

    $effect(() => (this.#any ? jobs.retain() : undefined));
    // a new file starts over, and is read once focus has rested on it
    $effect(() => {
      if (this.#key === null) return;
      this.report = undefined;
      this.error = "";
      this.large = false;
      const timer = setTimeout(() => void this.load(), SETTLE_MS);
      return () => clearTimeout(timer);
    });
    // the report is read again whenever an analysis finishes
    $effect(() => {
      if (jobs.finished) void untrack(() => this.load());
    });
    $effect(() => {
      const target = untrack(() => this.#target());
      const report = this.report;
      const layout = report?.spectrogram;
      if (!report || !layout || !target || report.stale) {
        this.scope = null;
        return;
      }
      const same = untrack(() => this.scope);
      if (same?.report.path === report.path && same.report.analyzedAt === report.analyzedAt) return;
      this.scope = new Scope(target, report, layout);
    });
    // the viewer's other pictures load when it opens, and it opens when it was asked to
    $effect(() => {
      const scope = this.scope;
      if (!scope) return;
      if (this.#openLarge === this.#key) {
        this.#openLarge = null;
        this.large = true;
      }
      if (this.large) void scope.pictures.start();
    });
    // asked for the analysis, a file without a report it can show is queued once that is known
    $effect(() => {
      const asked = inspector.asked;
      if (!asked || asked.key !== this.#key || this.report === undefined) return;
      inspector.asked = null;
      if (asked.large) this.#openLarge = asked.key;
      if (this.unread && session.can("ReadWrite")) void this.analyze(false);
    });
  }

  /** Reads the file's report, which the lists' chips follow */
  async load() {
    const target = this.#target();
    if (!target) return;
    const ask = ++this.#asked;
    const outcome = await loadReport(target);
    if (ask !== this.#asked) return;
    if (!outcome.ok) {
      this.error = outcome.error;
      return;
    }
    this.report = outcome.data;
    this.error = "";
    if (outcome.data) marks.put(outcome.data);
  }

  /** Queues the file, again with `force` when it has a report */
  async analyze(force: boolean) {
    const target = this.#target();
    if (!target) return;
    const outcome = await queue(target, force);
    // a file queued already shows its job once the queue is read
    if (outcome.ok || outcome.status === 409) void jobs.refresh();
    else toast(`Could not analyze the file: ${outcome.error}`, "error");
  }
}
