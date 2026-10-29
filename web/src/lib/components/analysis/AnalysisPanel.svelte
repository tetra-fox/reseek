<script lang="ts">
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import XIcon from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";

  import {
    DISCLAIMER,
    loadReport,
    queue,
    type Report,
    type Target,
    targetKey
  } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import { Scope } from "#lib/analysis/scope.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import { player, type Track } from "#lib/player.svelte.js";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import Judgement from "./Judgement.svelte";
  import Reading from "./Reading.svelte";
  import SpectrogramView from "./SpectrogramView.svelte";

  // one file's analysis, top to bottom: the spectrogram, the waveform and the spectrum at the
  // cursor with the disclaimer, the user's judgement, then the analyzer's reading folded away;
  // while the file waits or runs in the queue, how far it is with a way to cancel it
  type Props = {
    target: Target;
    /** the file as the player plays it, null when it is not one the player can be given */
    track?: Track | null;
    /** queue the file at once when it has no report yet, as the Analyze action does */
    start?: boolean;
    /** called once a file's report, or the lack of one, is on the page */
    onshown?: () => void;
  };

  let { target, track = null, start = false, onshown }: Props = $props();

  /** undefined while loading, null when the file has no report */
  let report = $state.raw<Report | null | undefined>(undefined);
  let error = $state("");
  let readingOpen = $state(false);
  const write = $derived(session.can("ReadWrite"));

  /** what the drawer and the large view show of the report, kept while the report is the same */
  let scope = $state.raw<Scope | null>(null);
  $effect(() => {
    const layout = report?.spectrogram;
    if (!report || !layout) {
      scope = null;
      return;
    }
    const same = untrack(() => scope);
    if (same?.report.path === report.path && same.report.analyzedAt === report.analyzedAt) return;
    const made = new Scope(target, report, layout);
    scope = made;
    void made.pictures.start();
  });

  // while the player plays this file, the cursor follows it once a frame
  $effect(() => {
    const shown = scope;
    const key = track?.key;
    if (!shown || !key || !player.playing || !player.holds(key)) return;
    let frame = requestAnimationFrame(function followed() {
      shown.played(player.position());
      frame = requestAnimationFrame(followed);
    });
    return () => cancelAnimationFrame(frame);
  });

  let asked = 0;
  const load = async () => {
    const ask = ++asked;
    const outcome = await loadReport(target);
    if (ask !== asked) return;
    if (outcome.ok) {
      report = outcome.data;
      // the lists' chips follow what the drawer shows, a judgement saved here included
      if (report) marks.put(report);
      error = "";
    } else {
      error = outcome.error;
    }
  };
  const analyze = async (force: boolean) => {
    const outcome = await queue(target, force);
    // a file queued already shows its job once the queue is read
    if (outcome.ok || outcome.status === 409) void jobs.refresh();
    else toast(`Couldn't analyze the file: ${outcome.error}`, "error");
  };

  $effect(() => jobs.retain());
  // a new file starts over
  $effect(() => {
    void targetKey(target);
    report = undefined;
    readingOpen = false;
    void load().then(() => tick().then(() => onshown?.()));
  });
  /** the file last queued because it was asked to start, so it is queued once */
  let started = "";
  // asked to start, a file without a report it can show is queued once that is known
  $effect(() => {
    const key = targetKey(target);
    // a report an older version made is as good as none
    const none = report === null || report?.stale === true;
    if (!start || !none || !write || started === key) return;
    started = key;
    void analyze(false);
  });
  /** the spectrogram shown across the whole screen */
  let large = $state(false);
  // the report is read again whenever an analysis finishes
  $effect(() => {
    if (jobs.finished) void load();
  });

  const job = $derived(jobs.of(target));
</script>

<div class="flex flex-col gap-3">
  {#if job}
    <div class="flex items-center gap-2 text-sm" role="status">
      <div class="flex flex-1 flex-col gap-1">
        <span>
          {job.state === "running"
            ? `Analyzing: ${Math.round((job.progress ?? 0) * 100)}%`
            : "Waiting for the analyzer"}
        </span>
        <div class="h-1.5 overflow-hidden rounded-full bg-surface-muted">
          <div
            class="h-full bg-accent transition-[width]"
            style:width={`${(job.progress ?? 0) * 100}%`}>
          </div>
        </div>
      </div>
      {#if write}
        <Button variant="outline" size="sm" onclick={() => void jobs.cancel(job.id)}
          ><XIcon />Cancel</Button>
      {/if}
    </div>
  {:else if report === null && write}
    <div class="flex flex-col items-start gap-2 text-sm">
      <p class="text-fg-muted">This file hasn't been analyzed yet.</p>
      <Button size="sm" onclick={() => void analyze(false)}>Analyze</Button>
    </div>
  {:else if report === null}
    <p class="text-sm text-fg-muted">This file hasn't been analyzed yet.</p>
  {/if}

  {#if error}
    <p class="text-sm text-fg-muted">{error}</p>
  {:else if report === undefined}
    <p class="text-sm text-fg-muted">Loading...</p>
  {:else if report?.stale}
    {#if !job}
      <div class="flex flex-col items-start gap-2 text-sm">
        <p class="text-fg-muted">
          This was analyzed by an older version of reseek. Analyze again to see it.
        </p>
        {#if write}
          <Button size="sm" onclick={() => void analyze(false)}>Analyze</Button>
        {/if}
      </div>
    {/if}
    <Judgement {target} {report} onsaved={() => void load()} />
  {:else if report}
    {#if scope}
      <SpectrogramView {scope} {track} onlarge={() => (large = true)} />
      <Dialog
        bind:open={large}
        title={report.relativePath.split("/").at(-1) ?? report.relativePath}
        class="w-[calc(100vw-2rem)] max-w-[120rem]">
        <SpectrogramView {scope} {track} large />
      </Dialog>
    {:else}
      <p class="text-sm">{report.summary}</p>
      <p
        class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
        {DISCLAIMER}
      </p>
    {/if}
    <Judgement {target} {report} onsaved={() => void load()} />
    <Reading {report} bind:open={readingOpen} />
    {#if write && !job}
      <Button variant="ghost" size="sm" class="self-start" onclick={() => void analyze(true)}
        ><RefreshCwIcon />Analyze again</Button>
    {/if}
  {/if}
</div>
