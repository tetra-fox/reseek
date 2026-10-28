<script lang="ts">
  import { Axis, Chart, Grid, Highlight, Html, Legend, Svg, Tooltip } from "layerchart";
  import { SvelteSet } from "svelte/reactivity";

  import DitherFill, { type AreaFill } from "#lib/components/charts/DitherFill.svelte";
  import { axisPadding, LABEL_REM, timeLabel, valueLabel } from "#lib/dashboard/chart.js";
  import type { Point } from "#lib/dashboard/reports.js";
  import type { Series } from "#lib/dashboard/series.js";
  import { rem } from "#lib/viewport.svelte.js";

  // transfer history as dithered areas over time on LayerChart, which lays out the axes, the
  // legend that shows and hides each series, and the tooltip of the bucket under the pointer
  type Props = {
    data: Point[];
    series: Series[];
    /** the series drawn, by key */
    visible: SvelteSet<string>;
    label: string;
  };

  let { data, series, visible, label }: Props = $props();

  const shown = $derived(series.filter((s) => visible.has(s.key)));
  const units = $derived(new Set(shown.map((s) => s.unit)));
  const format = $derived(valueLabel(units.size === 1 ? [...units][0] : null));
  const rows = $derived(data.map((p) => ({ ...p, at: new Date(p.timestamp) })));
  const xLabel = $derived(timeLabel(data[0]?.timestamp ?? 0, data.at(-1)?.timestamp ?? 0));
  const top = $derived(Math.max(0, ...data.flatMap((p) => shown.map((s) => p[s.key]))));
  // the axis labels' room, from the longest one they can show at the text size in use
  const padding = $derived(axisPadding([format(top), format(top * 1.25), format(0)], rem.px));
  const tickLabel = { class: "fill-fg-muted tabular-nums", style: `font-size: ${LABEL_REM}rem` };
  const toggle = (key: string) => (visible.has(key) ? visible.delete(key) : visible.add(key));

  /** What the chart says to a screen reader: each series' peak and when it was */
  const summary = $derived(
    `${label}: ${
      shown
        .map((s) => {
          const peak = data.reduce((a, b) => (b[s.key] > a[s.key] ? b : a), data[0]);
          return peak
            ? `${s.name} peaks at ${valueLabel(s.unit)(peak[s.key])}, ${xLabel(peak.timestamp)}`
            : s.name;
        })
        .join("; ") || "no series shown"
    }`
  );
</script>

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap gap-1" role="group" aria-label={`${label} series`}>
    {#each series as s (s.key)}
      <button
        type="button"
        aria-pressed={visible.has(s.key)}
        class={[
          "flex items-center gap-1.5 rounded-badge px-1.5 py-0.5 text-xs hover:bg-hover",
          !visible.has(s.key) && "opacity-35"
        ]}
        onclick={() => toggle(s.key)}>
        <span class="rounded-sm size-2.5" style:background-color={s.color}></span>{s.name}
      </button>
    {/each}
  </div>
  <div class="h-52" role="img" aria-label={summary}>
    <Chart
      data={rows}
      x="at"
      y={(d: (typeof rows)[number]) => shown.map((s) => d[s.key])}
      yDomain={[0, null]}
      yNice
      {padding}
      tooltipContext={{ mode: "bisect-x" }}>
      {#snippet children({ context })}
        {@const point = context.tooltipState.data}
        <Html pointerEvents={false}>
          <DitherFill
            width={context.width}
            height={context.height}
            areas={shown.map((s): AreaFill => ({
              key: s.key,
              color: s.color,
              x: rows.map((d) => context.xScale(d.at)),
              top: rows.map((d) => context.yScale(d[s.key])),
              floor: rows.map(() => context.yScale(0))
            }))}
            hovered={!!point}
            marker={point ? context.xScale(point.at) : null} />
        </Html>
        <Svg>
          <Grid y class="stroke-border" style="stroke-dasharray: 3 3" />
          <Axis placement="left" {format} tickSpacing={rem.px * 3} tickLabelProps={tickLabel} />
          <Axis
            placement="bottom"
            format={(t: Date) => xLabel(t.getTime())}
            tickSpacing={rem.px * 6}
            tickOcclusion
            tickLabelProps={tickLabel} />
          <Highlight lines={{ class: "stroke-fg-subtle" }} />
        </Svg>
        <Tooltip.Root>
          {#snippet children({ data: p })}
            <Tooltip.Header value={xLabel(p.timestamp)} />
            <Tooltip.List>
              {#each shown as s (s.key)}
                <Tooltip.Item label={s.name} color={s.color} value={valueLabel(s.unit)(p[s.key])} />
              {/each}
            </Tooltip.List>
          {/snippet}
        </Tooltip.Root>
      {/snippet}
    </Chart>
  </div>
</div>
