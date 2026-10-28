<script lang="ts" module>
  /** how wide the line is drawn, in pixels */
  export const SPARKLINE_WIDTH = 48;
</script>

<script lang="ts">
  import { Chart, Html } from "layerchart";

  import DitherFill from "#lib/components/charts/DitherFill.svelte";

  // the last speeds a transfer reported, as a small dithered area on LayerChart, still and
  // without axes; nothing until there are two
  let { points }: { points: number[] } = $props();

  const HEIGHT = 14;
  const data = $derived(points.map((speed, i) => ({ i, speed })));
</script>

{#if points.length >= 2}
  <div
    class="relative shrink-0"
    style:width={`${SPARKLINE_WIDTH}px`}
    style:height={`${HEIGHT}px`}
    aria-hidden="true">
    <Chart {data} x="i" y="speed" yDomain={[0, null]} padding={0} tooltipContext={false}>
      {#snippet children({ context })}
        <Html pointerEvents={false}>
          <DitherFill
            width={context.width}
            height={context.height}
            areas={[
              {
                key: "speed",
                color: "var(--color-accent)",
                x: data.map((d) => context.xScale(d.i)),
                top: data.map((d) => context.yScale(d.speed)),
                floor: data.map(() => context.yScale(0))
              }
            ]}
            animate={false}
            bloom={false} />
        </Html>
      {/snippet}
    </Chart>
  </div>
{/if}
