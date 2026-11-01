<script lang="ts">
  import AudioLinesIcon from "@lucide/svelte/icons/audio-lines";

  import { type Report, type Target, VERDICTS } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import { api } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import EmptyState from "#lib/components/ui/EmptyState.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Time from "#lib/components/ui/Time.svelte";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { getFileName } from "#lib/format.js";
  import { call } from "#lib/request.js";

  import AnalysisMark from "./AnalysisMark.svelte";

  // the files analyzed last, newest first, each one a click from a slot
  const SHOWN = 25;

  let reports = $state.raw<Report[] | null>(null);
  let error = $state("");

  const read = async () => {
    const outcome = await call(() =>
      api.GET("/api/v0/analysis", { params: { query: { limit: SHOWN } } })
    );
    if (outcome.ok) reports = outcome.data;
    error = outcome.ok ? "" : outcome.error;
  };
  $effect(() => jobs.retain());
  $effect(() => marks.retain());
  $effect(() => {
    void jobs.finished;
    void read();
  });

  const target = (r: Report): Target => ({ kind: "file", root: r.root, path: r.relativePath });
</script>

<section class="flex flex-col gap-2" aria-label="Recent results">
  <h2 class="text-sm font-medium">Recent results</h2>
  {#if error}
    <Notice kind="error">
      {error}
      {#snippet action()}
        <Button variant="ghost" size="sm" onclick={() => void read()}>Retry</Button>
      {/snippet}
    </Notice>
  {:else if !reports}
    <Skeleton class="h-24" />
  {:else if !reports.length}
    <EmptyState
      icon={AudioLinesIcon}
      title="No results yet"
      description="Files appear here once they have been analyzed." />
  {:else}
    <ul class="flex flex-col rounded-card border bg-surface p-1 shadow-card">
      {#each reports as report (report.path)}
        {@const name = getFileName(report.relativePath)}
        {@const mark = marks.file(report.root, report.relativePath)}
        <li class="flex items-center gap-1 pick-row pr-1">
          <button
            type="button"
            class="flex min-h-9 min-w-0 flex-1 items-center gap-2 px-2 text-left text-sm"
            title={report.relativePath}
            aria-label={`${A.openAnalysis.label}: ${name}`}
            onclick={() => slots.open(target(report), name)}>
            <span class="min-w-0 flex-1 truncate">{name}</span>
            <AnalysisMark mark={mark?.judgment ? mark : undefined} />
            <span class="shrink-0 text-xs text-fg-muted max-sm:hidden"
              >{report.stale ? "Analyzed by an older version" : VERDICTS[report.verdict]}</span>
            <span class="shrink-0 text-xs text-fg-muted max-md:hidden"
              ><Time at={report.analyzedAt} /></span>
          </button>
          {#if slots.occupied}
            <Button
              variant="ghost"
              size="sm"
              aria-label={`${A.compare.label}: ${name}`}
              onclick={() => slots.compare(target(report), name)}
              ><A.compare.icon />{A.compare.label}</Button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>
