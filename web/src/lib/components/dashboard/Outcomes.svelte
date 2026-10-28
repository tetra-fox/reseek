<script lang="ts">
  import { goto } from "$app/navigation";
  import { BarChart } from "layerchart";

  import type { Schemas } from "#lib/api/client.js";
  import { outcomes } from "#lib/dashboard/reports.js";
  import { href } from "#lib/nav.js";

  // how the range's transfers ended, each direction one stacked bar; a part opens the list of
  // those transfers
  let { summary }: { summary: Schemas["SummaryReport"] } = $props();

  const data = $derived(outcomes(summary));
  const SERIES = [
    { key: "succeeded", label: "Succeeded", color: "var(--color-success)", state: "completed" },
    { key: "failed", label: "Failed", color: "var(--color-danger)", state: "errored" },
    { key: "cancelled", label: "Cancelled", color: "var(--color-fg-subtle)", state: "cancelled" }
  ] as const;
  const total = $derived(data.reduce((n, d) => n + d.succeeded + d.failed + d.cancelled, 0));

  const open = (direction: string, key: string) => {
    const state = SERIES.find((s) => s.key === key)?.state;
    const list = direction === "Downloads" ? "/downloads" : "/uploads";
    void goto(href(`${list}?state=${state}`));
  };
</script>

<div class="flex flex-col gap-3">
  <div class="flex flex-wrap items-center gap-3 text-xs text-fg-muted" aria-hidden="true">
    {#each SERIES as s (s.key)}
      <span class="flex items-center gap-1.5"
        ><span class="rounded-sm size-2.5" style:background-color={s.color}></span>{s.label}</span>
    {/each}
  </div>
  {#if total}
    <div class="h-28" role="img" aria-label="Transfer outcomes">
      <BarChart
        {data}
        y="direction"
        orientation="horizontal"
        series={SERIES.map((s) => ({
          key: s.key,
          label: s.label,
          color: s.color,
          props: { strokeWidth: 0, class: "cursor-pointer" }
        }))}
        seriesLayout="stack"
        bandPadding={0.35}
        onBarClick={(_, { data: row, series }) => open(row.direction, series.key)} />
    </div>
  {:else}
    <p class="text-sm text-fg-subtle">No transfers finished in this range.</p>
  {/if}
  <!-- the same figures as links, for the keyboard and for reading exactly: a column for each
       direction, each figure whole on a line of its own -->
  <dl class="grid grid-cols-2 gap-x-4 text-sm">
    {#each data as d (d.direction)}
      <div class="flex flex-col gap-0.5">
        <dt class="text-fg-muted">{d.direction}</dt>
        {#each SERIES as s (s.key)}
          <dd>
            <a
              class="whitespace-nowrap tabular-nums underline-offset-2 hover:underline"
              href={href(
                `${d.direction === "Downloads" ? "/downloads" : "/uploads"}?state=${s.state}`
              )}
              >{d[s.key].toLocaleString()}
              <span class="text-fg-muted">{s.label.toLowerCase()}</span></a>
          </dd>
        {/each}
      </div>
    {/each}
  </dl>
</div>
