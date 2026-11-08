<script lang="ts">
  import { type Report, type Target, VERDICTS } from "#lib/analysis/analysis.js";
  import { jobs } from "#lib/analysis/jobs.svelte.js";
  import { marks } from "#lib/analysis/marks.svelte.js";
  import { slots } from "#lib/analysis/slots.svelte.js";
  import { api } from "#lib/api/client.js";
  import Button from "#lib/components/ui/Button.svelte";
  import MiddleTruncate from "#lib/components/ui/MiddleTruncate.svelte";
  import Notice from "#lib/components/ui/Notice.svelte";
  import Skeleton from "#lib/components/ui/Skeleton.svelte";
  import Time from "#lib/components/ui/Time.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { FILE_ACTIONS as A } from "#lib/files/actions.js";
  import { getFileName } from "#lib/format.js";
  import { call } from "#lib/request.js";

  import AnalysisMark from "./AnalysisMark.svelte";

  // the files analyzed last, newest first, each one a click from the first slot, under its empty
  // placeholder. The list scrolls on its own, in what room the row leaves it
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

<section
  class="flex min-h-0 w-full max-w-2xl flex-col gap-1 max-md:max-h-80"
  aria-label="Recent results">
  <h2 class="shrink-0 text-sm font-medium">Recent results</h2>
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
    <p class="text-sm text-fg-muted">No files analyzed yet</p>
  {:else}
    <ul class="flex min-h-0 flex-col overflow-y-auto">
      {#each reports as report (report.path)}
        {@const name = getFileName(report.relativePath)}
        {@const mark = marks.file(report.root, report.relativePath)}
        <li class="pick-row">
          <Tooltip text={report.relativePath} dense>
            {#snippet child({ props })}
              <button
                {...props}
                type="button"
                class="flex min-h-9 w-full min-w-0 items-center gap-2 px-2 text-left text-sm"
                aria-label={`${A.openAnalysis.label}: ${name}`}
                onclick={() => slots.open(target(report), name)}>
                <MiddleTruncate text={name} class="flex-1" />
                <AnalysisMark mark={mark?.judgment ? mark : undefined} />
                <span class="shrink-0 text-xs text-fg-muted max-sm:hidden"
                  >{report.stale ? "Analyzed by an older version" : VERDICTS[report.verdict]}</span>
                <span class="shrink-0 text-xs text-fg-muted max-md:hidden"
                  ><Time at={report.analyzedAt} /></span>
              </button>
            {/snippet}
          </Tooltip>
        </li>
      {/each}
    </ul>
  {/if}
</section>
