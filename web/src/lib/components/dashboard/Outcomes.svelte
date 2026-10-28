<script lang="ts">
  import { goto } from "$app/navigation";
  import { Axis, Chart, Grid, Html, Svg, Tooltip } from "layerchart";

  import type { Schemas } from "#lib/api/client.js";
  import DitherFill, { type BarFill } from "#lib/components/charts/DitherFill.svelte";
  import { axisPadding, tickLabel } from "#lib/dashboard/chart.js";
  import { outcomes } from "#lib/dashboard/reports.js";
  import { href } from "#lib/nav.js";
  import { rem } from "#lib/viewport.svelte.js";

  // how the range's transfers ended, each direction one stacked bar of dither on LayerChart; a
  // part opens the list of those transfers
  let { summary }: { summary: Schemas["SummaryReport"] } = $props();

  const data = $derived(outcomes(summary));
  const SERIES = [
    { key: "succeeded", label: "Succeeded", color: "var(--color-success)", state: "completed" },
    { key: "failed", label: "Failed", color: "var(--color-danger)", state: "errored" },
    { key: "cancelled", label: "Cancelled", color: "var(--color-fg-subtle)", state: "cancelled" }
  ] as const;
  type Row = (typeof data)[number];
  const total = $derived(data.reduce((n, d) => n + d.succeeded + d.failed + d.cancelled, 0));
  // the directions' names on the left, the counts below
  const padding = $derived({
    ...axisPadding(["Downloads", "Uploads"], rem.px),
    top: 0
  });

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
        x={(d: Row) => d.succeeded + d.failed + d.cancelled}
        xDomain={[0, null]}
        xNice
        y="direction"
        bandPadding={0.35}
        valueAxis="x"
        {padding}
        tooltipContext={{ mode: "band" }}>
        {#snippet children({ context })}
          {@const hovered = context.tooltipState.data as Row | null}
          {@const band = context.yScale.bandwidth?.() ?? 0}
          <Html pointerEvents={false}>
            <DitherFill
              width={context.width}
              height={context.height}
              bars={data.flatMap((d, i) =>
                parts(d).map((p): BarFill => ({
                  color: p.series.color,
                  x0: context.xScale(p.from),
                  x1: context.xScale(p.to),
                  top: context.yScale(d.direction),
                  floor: context.yScale(d.direction) + band,
                  group: i
                }))
              )}
              hovered={!!hovered}
              group={hovered ? data.indexOf(hovered) : null} />
          </Html>
          <Svg>
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
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <rect
                  x={context.xScale(p.from)}
                  y={context.yScale(d.direction)}
                  width={Math.max(0, context.xScale(p.to) - context.xScale(p.from))}
                  height={band}
                  fill="transparent"
                  class="cursor-pointer"
                  role="presentation"
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
