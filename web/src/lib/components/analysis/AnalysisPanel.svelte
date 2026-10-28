<script lang="ts">
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import XIcon from "@lucide/svelte/icons/x";
  import { tick, untrack } from "svelte";

  import {
    loadReport,
    queue,
    type Report,
    type Target,
    targetKey
  } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import { spectrogramView } from "#lib/analysis/view.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import { session } from "#lib/session.svelte.js";
  import { toast } from "#lib/toast.svelte.js";

  import Judgement from "./Judgement.svelte";
  import Reading from "./Reading.svelte";
  import Spectrogram, { DISCLAIMER } from "./Spectrogram.svelte";
  import WindowSpectrum from "./WindowSpectrum.svelte";
  import WindowStrip from "./WindowStrip.svelte";

  // one file's analysis, top to bottom: the spectrogram with its guide lines and the disclaimer,
  // the user's judgement, then the analyser's reading folded away; while the file waits or runs
  // in the queue, how far it is with a way to cancel it
  type Props = {
    target: Target;
    /** queue the file at once when it has no report yet, as the Analyse action does */
    start?: boolean;
    /** called once a file's report, or the lack of one, is on the page */
    onshown?: () => void;
  };

  let { target, start = false, onshown }: Props = $props();

  /** undefined while loading, null when the file has no report */
  let report = $state.raw<Report | null | undefined>(undefined);
  let error = $state("");
  let picked = $state<number | null>(null);
  let readingOpen = $state(false);
  let windowsShown = $state<HTMLElement | null>(null);
  const write = $derived(session.can("ReadWrite"));

  let asked = 0;
  const load = async () => {
    const ask = ++asked;
    // the scale is read here but reloads through its own effect, which keeps the picked window
    const outcome = await loadReport(
      target,
      untrack(() => spectrogramView.scale)
    );
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
  const analyse = async (force: boolean) => {
    const outcome = await queue(target, force);
    // a file queued already shows its job once the queue is read
    if (outcome.ok || outcome.status === 409) void jobs.refresh();
    else toast(`Couldn't analyse the file: ${outcome.error}`, "error");
  };

  $effect(() => jobs.retain());
  // a new file starts over
  $effect(() => {
    void targetKey(target);
    report = undefined;
    picked = null;
    readingOpen = false;
    void load().then(() => tick().then(() => onshown?.()));
  });
  /** the file last queued because it was asked to start, so it is queued once */
  let started = "";
  // asked to start, a file without a report is queued once it is known to have none
  $effect(() => {
    const key = targetKey(target);
    if (!start || report !== null || !write || started === key) return;
    started = key;
    void analyse(false);
  });
  // another scale is the same report with its layout on that scale
  $effect(() => {
    void spectrogramView.scale;
    if (untrack(() => report)) void load();
  });
  /** the spectrogram shown across the whole screen */
  let large = $state(false);
  // the report is read again whenever an analysis finishes
  $effect(() => {
    if (jobs.finished) void load();
  });

  const job = $derived(jobs.of(target));
  const starts = $derived(report?.windows?.map((w) => w.t) ?? []);
  /** A window picked on the spectrogram opens the reading at that window's spectrum */
  const pickOnPlot = async (index: number) => {
    picked = index;
    readingOpen = true;
    await tick();
    // the spectrum loads under the strip, so the strip goes to the top to leave room for it
    windowsShown?.scrollIntoView({ block: "start" });
  };
</script>

<div class="flex flex-col gap-3">
  {#if job}
    <div class="flex items-center gap-2 text-sm" role="status">
      <div class="flex flex-1 flex-col gap-1">
        <span>
          {job.state === "running"
            ? `Analysing: ${Math.round((job.progress ?? 0) * 100)}%`
            : "Waiting for the analyser"}
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
      <p class="text-fg-muted">This file hasn't been analysed yet.</p>
      <Button size="sm" onclick={() => void analyse(false)}>Analyse</Button>
    </div>
  {:else if report === null}
    <p class="text-sm text-fg-muted">This file hasn't been analysed yet.</p>
  {/if}

  {#if error}
    <p class="text-sm text-fg-muted">{error}</p>
  {:else if report === undefined}
    <p class="text-sm text-fg-muted">Loading...</p>
  {:else if report}
    {#if report.spectrogram}
      <Spectrogram
        {target}
        layout={report.spectrogram}
        {starts}
        {picked}
        onpick={(i) => void pickOnPlot(i)}
        onlarge={() => (large = true)} />
      <Dialog
        bind:open={large}
        title={report.relativePath.split("/").at(-1) ?? report.relativePath}
        class="w-[calc(100vw-2rem)] max-w-[120rem]">
        <Spectrogram
          {target}
          layout={report.spectrogram}
          {starts}
          {picked}
          onpick={(i) => (picked = i)} />
      </Dialog>
    {:else}
      <p class="text-sm">{report.summary}</p>
      <p
        class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
        {DISCLAIMER}
      </p>
    {/if}
    <Judgement {target} {report} onsaved={() => void load()} />
    <Reading {report} bind:open={readingOpen}>
      {#if report.spectrogram && report.windows?.length}
        <div class="flex flex-col gap-1" bind:this={windowsShown}>
          <h4 class="text-xs font-medium text-fg-muted uppercase">Each window</h4>
          <p class="text-xs text-fg-muted">
            How high each second's content reaches. Pick one to see its spectrum.
          </p>
          <WindowStrip
            layout={report.spectrogram}
            windows={report.windows}
            {picked}
            onpick={(i) => (picked = i)} />
          {#if picked !== null}
            <WindowSpectrum
              {target}
              layout={report.spectrogram}
              {starts}
              {picked}
              onpick={(i) => (picked = i)} />
          {/if}
        </div>
      {/if}
    </Reading>
    {#if write && !job}
      <Button variant="ghost" size="sm" class="self-start" onclick={() => void analyse(true)}
        ><RefreshCwIcon />Analyse again</Button>
    {/if}
  {/if}
</div>
