<script lang="ts">
  import Badge from "#lib/components/ui/Badge.svelte";
  import Spinner from "#lib/components/ui/Spinner.svelte";
  import Tooltip from "#lib/components/ui/Tooltip.svelte";
  import { columnWidth } from "#lib/format.js";
  import type { Job, JobPart } from "#lib/jobs/jobs.js";
  import { jobs } from "#lib/jobs/jobs.svelte.js";
  import { ListWindow } from "#lib/lists/virtual.svelte.js";
  import { href } from "#lib/nav.js";
  import { rem } from "#lib/ui/viewport.svelte.js";

  // an opened job's parts, read as it opens and again as the job changes
  let { job }: { job: Job } = $props();

  let parts = $state.raw<JobPart[] | null>(null);
  let partsError = $state<string | null>(null);
  $effect(() => {
    void job.counts;
    void job.state;
    let current = true;
    void jobs.detail(job.id).then((outcome) => {
      if (!current) return;
      if (outcome.ok) {
        parts = outcome.data.parts;
        partsError = null;
      } else partsError = outcome.error;
    });
    return () => (current = false);
  });

  const tone = (p: JobPart) =>
    p.state === "succeeded"
      ? "success"
      : p.state === "failed"
        ? "danger"
        : p.state === "running"
          ? "accent"
          : "neutral";
  const STATES: Record<JobPart["state"], string> = {
    waiting: "Waiting",
    running: "Running",
    paused: "Paused",
    succeeded: "Succeeded",
    failed: "Failed",
    canceled: "Canceled"
  };
  // the badges' column holds the longest state, so the names line up
  const STATE_WIDTH = columnWidth(STATES.succeeded);

  // two parts can share a name, so a part is known by its place
  const lines = $derived((parts ?? []).map((part, i) => ({ part, key: `${i}:${part.name}` })));
  // a job can have thousands of parts; the window draws those in view
  const view = new ListWindow(
    () => lines,
    (line) => line.key,
    () => ({ estimateSize: () => 1.375 * rem.px, overscan: 10, sizes: rem.px })
  );
</script>

{#if partsError}
  <span class="text-danger">{partsError}</span>
{:else if parts === null}
  <Spinner class="size-4" />
{:else if !parts.length}
  <span class="text-fg-muted">This job has no parts</span>
{:else}
  <div class="max-h-80 overflow-y-auto" {@attach view.attach}>
    <ul class="relative" style:height={`${view.total}px`}>
      {#each view.drawn as { line: { part: p }, key, index, start } (key)}
        <!-- the error goes under the name, so neither is cut to a few letters on a phone -->
        <li
          data-index={index}
          {@attach view.measure}
          class="absolute inset-x-0 top-0 grid items-center gap-x-2 py-0.5"
          style:grid-template-columns={`${STATE_WIDTH} minmax(0, 1fr)`}
          style:transform={`translateY(${start}px)`}>
          <Badge tone={tone(p)} class="justify-self-start">{STATES[p.state]}</Badge>
          {#if p.transferId}
            <Tooltip text={p.name} clipped>
              {#snippet child({ props })}
                <a
                  {...props}
                  class="truncate hover:underline"
                  href={href(`/downloads?job=${job.id}&transfer=${p.transferId}`)}>{p.name}</a>
              {/snippet}
            </Tooltip>
          {:else}
            <Tooltip text={p.name} clipped>
              {#snippet child({ props })}<span {...props} class="truncate">{p.name}</span>{/snippet}
            </Tooltip>
          {/if}
          {#if p.error}
            <Tooltip text={p.error} clipped>
              {#snippet child({ props })}
                <span {...props} class="col-start-2 truncate text-danger">{p.error}</span>
              {/snippet}
            </Tooltip>
          {/if}
        </li>
      {/each}
    </ul>
  </div>
{/if}
