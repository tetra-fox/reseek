<script lang="ts">
  import { Area, Chart, Svg } from "layerchart";

  import { chartCurve } from "#lib/dashboard/chart.js";
  import { SPARKLINE_WIDTH } from "#lib/transfers/grid/columns.js";

  // the last speeds a transfer reported, as a small area on LayerChart, still and without axes;
  // nothing until there are two
  let { points }: { points: number[] } = $props();

  const HEIGHT = 14;
  const data = $derived(points.map((speed, i) => ({ i, speed })));
</script>

{#if points.length >= 2}
  <div
    class="shrink-0"
    style:width={`${SPARKLINE_WIDTH}px`}
    style:height={`${HEIGHT}px`}
    aria-hidden="true">
    <Chart {data} x="i" y="speed" yDomain={[0, null]} padding={0} tooltipContext={false}>
      <Svg>
        <Area
          fill="var(--color-accent)"
          fillOpacity={0.2}
          curve={chartCurve}
          line={{ stroke: "var(--color-accent)", strokeWidth: 1.25 }} />
      </Svg>
    </Chart>
  </div>
{/if}
