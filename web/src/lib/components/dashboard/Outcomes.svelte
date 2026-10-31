<script lang="ts">
  import { goto } from "$app/navigation";
  import { Axis, Bar, Chart, Grid, Svg, Tooltip } from "layerchart";

  import type { Schemas } from "#lib/api/client.js";
  import EmptyRow from "#lib/components/ui/EmptyRow.svelte";
  import { axisPadding, tickLabel } from "#lib/dashboard/chart.js";
  import { outcomes } from "#lib/dashboard/reports.js";
  import { href } from "#lib/nav.js";
  import { chartMotion } from "#lib/ui/motion.js";
  import { rem } from "#lib/ui/viewport.svelte.js";

  // how the range's transfers ended, each direction one stacked bar on LayerChart; a part opens
  // the list of those transfers, and a hover fades the other direction
  let { summary }: { summary: Schemas["SummaryReport"] } = $props();

  const data = $derived(outcomes(summary));
  const SERIES = [
    { key: "succeeded", label: "Succeeded", color: "var(--color-success)", state: "completed" },
    { key: "failed", label: "Failed", color: "var(--color-danger)", state: "errored" },
    { key: "canceled", label: "Canceled", color: "var(--color-fg-subtle)", state: "canceled" }
  ] as const;
  type Row = (typeof data)[number];
  const total = $derived(data.reduce((n, d) => n + d.succeeded + d.failed + d.canceled, 0));
  // the directions' names on the left, the counts below
  const padding = $derived({
    ...axisPadding(["Downloads", "Uploads"], rem.px),
    top: 0
  });

  const motion = chartMotion();

  /** Each part of each bar: its series and where it lies in counts */
  const parts = (d: Row) => {
    let at = 0;
    return SERIES.map((s) => {
      const from = at;
      at += d[s.key];
      return { series: s, from, to: at };
    });
  };

  const open = (direction: string, state: string) => {
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
    <div
      class="h-28"
      role="img"
      aria-label={`Transfer outcomes: ${data
        .map(
          (d) =>
            `${d.direction} ${SERIES.map((s) => `${d[s.key]} ${s.label.toLowerCase()}`).join(", ")}`
        )
        .join("; ")}`}>
      <Chart
        {data}
        x={(d: Row) => d.succeeded + d.failed + d.canceled}
        xDomain={[0, null]}
        xNice
        y="direction"
        bandPadding={0.35}
        valueAxis="x"
        {padding}
        tooltipContext={{ mode: "band" }}>
        {#snippet children({ context })}
          {@const hovered = context.tooltipState.data as Row | null}
          <!-- the marks tween to a new size, the box does not, so they are cut at its edge -->
          <Svg clip>
            <Grid x class="stroke-border" />
            <Axis placement="left" tickLabelProps={tickLabel} />
            <Axis
              placement="bottom"
              format={(n: number) => n.toLocaleString()}
              tickSpacing={rem.px * 4}
              tickOcclusion
              tickLabelProps={tickLabel} />
            <!-- each part takes a click, which opens the list of those transfers -->
            {#each data as d (d.direction)}
              {#each parts(d) as p (p.series.key)}
                <Bar
                  data={d}
                  x={() => [p.from, p.to]}
                  fill={p.series.color}
                  opacity={hovered && hovered !== d ? 0.45 : 1}
                  class="cursor-pointer transition-opacity"
                  {motion}
                  onclick={() => open(d.direction, p.series.state)} />
              {/each}
            {/each}
          </Svg>
          <Tooltip.Root>
            {#snippet children({ data: d })}
              <Tooltip.Header value={d.direction} />
              <Tooltip.List>
                {#each SERIES as s (s.key)}
                  <Tooltip.Item label={s.label} color={s.color} value={d[s.key].toLocaleString()} />
                {/each}
              </Tooltip.List>
            {/snippet}
          </Tooltip.Root>
        {/snippet}
      </Chart>
    </div>
  {:else}
    <EmptyRow text="No transfers finished in this range" />
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
