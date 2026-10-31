<script lang="ts">
  import Maximize2Icon from "@lucide/svelte/icons/maximize-2";
  import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
  import XIcon from "@lucide/svelte/icons/x";

  import { DISCLAIMER, type Target, VERDICTS } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import type { FileReport } from "#lib/analysis/report.svelte.js";
  import Button from "#lib/components/ui/Button.svelte";
  import Dialog from "#lib/components/ui/Dialog.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import { player, type Track } from "#lib/files/player.svelte.js";
  import { session } from "#lib/session.svelte.js";

  import Judgment from "./Judgment.svelte";
  import Reading from "./Reading.svelte";
  import SpectrogramView from "./SpectrogramView.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  // a file's analysis in the inspector: how far it is in the queue, the verdict in a line and the
  // spectrogram small, with everything else in the large view: the spectrogram with its controls,
  // the user's judgment and the analyzer's reading
  type Props = {
    analyzed: FileReport;
    target: Target;
    /** the file as the player plays it */
    track: Track | null;
    name: string;
  };

  let { analyzed, target, track, name }: Props = $props();

  const report = $derived(analyzed.report);
  const scope = $derived(analyzed.scope);
  const job = $derived(analyzed.job);
  const write = $derived(session.can("ReadWrite"));
  const openLarge = () => (analyzed.large = true);

  // while the large view shows the file and the player plays it, the cursor follows it a frame
  // at a time
  $effect(() => {
    const shown = scope;
    const key = track?.key;
    if (!analyzed.large || !shown || !key || !player.playing || !player.holds(key)) return;
    let frame = requestAnimationFrame(function followed() {
      shown.played(player.position());
      frame = requestAnimationFrame(followed);
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

<div class="flex flex-col gap-2">
  {#if job}
    <div class="flex items-center gap-2" role="status">
      <div class="flex flex-1 flex-col gap-1">
        <span>
          {job.state === "running"
            ? `Analyzing: ${Math.round((job.progress ?? 0) * 100)}%`
            : "Waiting in the analysis queue"}
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
  {/if}

  {#if analyzed.error}
    <p class="text-fg-muted">{analyzed.error}</p>
  {:else if report === undefined}
    <Skeleton class="h-20" />
  {:else if report === null}
    {#if !job}<p class="text-fg-muted">This file has not been analyzed.</p>{/if}
  {:else if report.stale}
    {#if !job}
      <p class="text-fg-muted">
        This file was analyzed by an older version of reseek. Analyze it again to view the results.
      </p>
    {/if}
  {:else}
    <p>
      <span class="font-medium">{VERDICTS[report.verdict]}</span>
      <span class="text-fg-muted">· {Math.round(report.confidence * 100)}% confidence</span>
    </p>
    {#if scope}<Thumbnail {scope} onopen={openLarge} />{/if}
    <p class="text-xs text-fg-muted">An aid to judgment, not proof.</p>
  {/if}

  {#if report}
    <Button variant="outline" size="sm" class="self-start" onclick={openLarge}
      ><Maximize2Icon />Open large</Button>
    <Dialog bind:open={analyzed.large} title={name} class="w-[calc(100vw-2rem)] max-w-[120rem]">
      {#if scope}
        <SpectrogramView {scope} {track} />
      {:else if !report.stale}
        <p class="text-sm">{report.summary}</p>
        <p
          class="rounded-control border border-warning/40 bg-warning-soft p-2 text-sm text-warning-soft-fg">
          {DISCLAIMER}
        </p>
      {/if}
      <Judgment {target} {report} onsaved={() => void analyzed.load()} />
      {#if !report.stale}
        <Reading {report} />
        {#if write && !job}
          <Button
            variant="ghost"
            size="sm"
            class="self-start"
            onclick={() => void analyzed.analyze(true)}><RefreshCwIcon />Analyze again</Button>
        {/if}
      {/if}
    </Dialog>
  {/if}
</div>
