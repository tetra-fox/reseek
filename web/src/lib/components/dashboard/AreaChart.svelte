<script lang="ts">
  import { Area, Axis, Chart, Grid, Highlight, LinearGradient, Svg, Tooltip } from "layerchart";

  import {
    axisPadding,
    axisTicks,
    chartCurve,
    tickLabel,
    timeLabel,
    valueLabel
  } from "#lib/dashboard/chart.js";
  import type { Direction, Point } from "#lib/dashboard/reports.js";
  import type { ChartView, Series } from "#lib/dashboard/series.js";
  import { chartMotion } from "#lib/ui/motion.js";
  import { rem } from "#lib/ui/viewport.svelte.js";

  // transfer history as areas over time on LayerChart, each a smooth line over a fill that fades
  // toward the floor, with the axes in the view's one unit and the tooltip of the bucket under the
  // pointer; a view with both directions has a toggle for each, and the last one on stays on
  type Props = {
    data: Point[];
    view: ChartView;
    /** the directions turned off */
    hidden: Direction[];
    label: string;
  };

  let { data, view, hidden = $bindable(), label }: Props = $props();

  const shown = $derived(view.series.filter((s) => !s.direction || !hidden.includes(s.direction)));
  const format = $derived(valueLabel(view.unit));
  const rows = $derived(data.map((p) => ({ ...p, at: new Date(p.timestamp) })));
  const xLabel = $derived(timeLabel(data[0]?.timestamp ?? 0, data.at(-1)?.timestamp ?? 0));
  const top = $derived(Math.max(0, ...data.flatMap((p) => shown.map((s) => p[s.key]))));
  const ticks = $derived(axisTicks(view.unit, top));
  const padding = $derived(
    axisPadding(
      ticks.map(format),
      rem.px,
      [data[0], data.at(-1)].flatMap((p) => (p ? [xLabel(p.timestamp)] : []))
    )
  );
  const motion = chartMotion();
  const toggle = (direction: Direction) =>
    (hidden = hidden.includes(direction)
      ? hidden.filter((d) => d !== direction)
      : [...hidden, direction]);
  /** the one direction left on, which can't be turned off */
  const lastOn = $derived.by(() => {
    const on = shown.filter((s) => s.direction);
    return on.length === 1 ? on[0].direction : undefined;
  });

  /**
   * What the chart says to a screen reader: each series' peak and when it was, or that it stays
   * on the floor
   */
  const summary = $derived(
    `${label}: ${shown
      .map((s) => {
        const peak = data.reduce((a, b) => (b[s.key] > a[s.key] ? b : a), data[0]);
        if (!peak) return s.name;
        if (peak[s.key] <= 0) return `${s.name} flat at ${format(0)}`;
        return `${s.name} peaks at ${format(peak[s.key])}, ${xLabel(peak.timestamp)}`;
      })
      .join("; ")}`
  );
</script>

{#snippet swatch(s: Series)}
  <span class="rounded-sm size-2.5" style:background-color={s.color}></span>
{/snippet}

<div class="flex flex-col gap-2">
  <div class="flex flex-wrap gap-1" role="group" aria-label={`${label} series`}>
    {#each view.series as s (s.key)}
      {@const direction = s.direction}
      {#if direction}
        <button
          type="button"
          aria-pressed={!hidden.includes(direction)}
          disabled={direction === lastOn}
          title={direction === lastOn ? "One direction always stays on" : undefined}
          class={[
            "flex items-center gap-1.5 rounded-badge px-1.5 py-0.5 text-xs not-disabled:hover:bg-hover",
            hidden.includes(direction) && "opacity-35"
          ]}
          onclick={() => toggle(direction)}>
          {@render swatch(s)}{direction}
        </button>
      {:else}
        <span class="flex items-center gap-1.5 px-1.5 py-0.5 text-xs">
          {@render swatch(s)}{s.name}
        </span>
      {/if}
    {/each}
  </div>
  <div class="h-52" role="img" aria-label={summary}>
    <Chart
      data={rows}
      x="at"
      y={(d: (typeof rows)[number]) => shown.map((s) => d[s.key])}
      yDomain={[0, ticks.at(-1) || 1]}
      {padding}
      tooltipContext={{ mode: "bisect-x" }}>
      <!-- the marks tween to a new size, the box does not, so they are cut at its edge -->
      <Svg clip>
        <Grid y yTicks={ticks} class="stroke-border" style="stroke-dasharray: 3 3" />
        <Axis placement="left" {format} {ticks} tickLabelProps={tickLabel} />
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
              <Tooltip.Item label={s.name} color={s.color} value={format(p[s.key])} />
            {/each}
          </Tooltip.List>
        {/snippet}
      </Tooltip.Root>
    </Chart>
  </div>
</div>
