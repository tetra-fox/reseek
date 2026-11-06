<script lang="ts">
  import { NOT_ANALYZED, STALE, type Target, VERDICTS } from "#lib/analysis/analysis.js";
  import type { FileReport } from "#lib/analysis/report.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";

  import AnalysisProgress from "./AnalysisProgress.svelte";
  import Judgment from "./Judgment.svelte";
  import Reading from "./Reading.svelte";
  import Thumbnail from "./Thumbnail.svelte";

  // a file's analysis in the inspector: how far it is in the queue, the verdict in a line, the
  // spectrogram small, and the user's judgment and the analyzer's reading. Everything else is on
  // the Analysis page, which the small spectrogram opens
  type Props = { analyzed: FileReport; target: Target; name: string };

  let { analyzed, target, name }: Props = $props();

  const report = $derived(analyzed.report);
  const scope = $derived(analyzed.scope);
  const job = $derived(analyzed.job);
</script>

<div class="flex flex-col gap-2">
  {#if job}<AnalysisProgress {job} />{/if}

  {#if analyzed.error}
    <p class="text-fg-muted">{analyzed.error}</p>
  {:else if report === undefined}
    <Skeleton class="h-20" />
  {:else if report === null}
    {#if !job}<p class="text-fg-muted">{NOT_ANALYZED}</p>{/if}
  {:else if report.stale}
    {#if !job}<p class="text-fg-muted">{STALE}</p>{/if}
  {:else}
    <p>
      <span class="font-medium">{VERDICTS[report.verdict]}</span>
      <span class="text-fg-muted">· {Math.round(report.confidence * 100)}% confidence</span>
    </p>
    {#if scope}<Thumbnail {scope} onopen={() => slots.open(target, name)} />{/if}
    <p class="text-xs text-fg-muted">A guide, not proof</p>
  {/if}

  {#if report}
    <Judgment {target} {report} onsaved={() => void analyzed.load()} />
    {#if !report.stale}<Reading {report} />{/if}
  {/if}
</div>
