<script lang="ts">
  import { Area, Axis, Chart, Grid, Highlight, LinearGradient, Svg, Tooltip } from "layerchart";
  import { SvelteSet } from "svelte/reactivity";

  import {
    axisPadding,
    chartCurve,
    tickLabel,
    timeLabel,
    valueLabel
  } from "#lib/dashboard/chart.js";
  import type { Point } from "#lib/dashboard/reports.js";
  import type { Series } from "#lib/dashboard/series.js";
  import { chartMotion } from "#lib/ui/motion.js";
  import { rem } from "#lib/ui/viewport.svelte.js";

  // transfer history as areas over time on LayerChart, each a smooth line over a fill that fades
  // toward the floor, with the axes and the tooltip of the bucket under the pointer; the buttons
  // above show and hide each series
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
  const { motion, watch } = chartMotion();
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
  <div class="h-52" role="img" aria-label={summary} {@attach watch}>
    <Chart
      data={rows}
      x="at"
      y={(d: (typeof rows)[number]) => shown.map((s) => d[s.key])}
      yDomain={[0, null]}
      yNice
      {padding}
      tooltipContext={{ mode: "bisect-x" }}>
      <Svg>
        <Grid y class="stroke-border" style="stroke-dasharray: 3 3" />
        <Axis placement="left" {format} tickSpacing={rem.px * 3} tickLabelProps={tickLabel} />
        <Axis
          placement="bottom"
          format={(t: Date) => xLabel(t.getTime())}
          tickSpacing={rem.px * 6}
          tickOcclusion
          tickLabelProps={tickLabel} />
        {#each shown as s (s.key)}
          <LinearGradient vertical>
            {#snippet stopsContent()}
              <stop offset="0%" stop-color={s.color} stop-opacity="0.32" />
              <stop offset="100%" stop-color={s.color} stop-opacity="0.02" />
            {/snippet}
            {#snippet children({ gradient })}
              <Area
                y1={(d: (typeof rows)[number]) => d[s.key]}
                fill={gradient}
                curve={chartCurve}
                line={{ stroke: s.color, strokeWidth: 1.5 }}
                {motion} />
            {/snippet}
          </LinearGradient>
        {/each}
        <Highlight lines={{ class: "stroke-fg-subtle" }} />
        {#each shown as s (s.key)}
          <Highlight y={(d: (typeof rows)[number]) => d[s.key]} points={{ fill: s.color, r: 3 }} />
        {/each}
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
    </Chart>
  </div>
</div>
