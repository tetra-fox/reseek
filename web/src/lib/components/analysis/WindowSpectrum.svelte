<script lang="ts">
  import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import { Axis, Chart, Grid, Spline, Svg } from "layerchart";

  import {
    heightOf,
    type Layout,
    loadWindow,
    type Spectrum,
    type Target
  } from "#lib/analysis/analysis.js";
  import Button from "#lib/components/ui/Button.svelte";
  import { tickLabel } from "#lib/dashboard/chart.js";
  import { formatDuration } from "#lib/format.js";

  // one window's spectrum on LayerChart: level against frequency, on the frequency axis the
  // spectrogram uses, with the window before and after a press away
  type Props = {
    target: Target;
    layout: Layout;
    starts: number[];
    picked: number;
    onpick: (index: number) => void;
  };

  let { target, layout, starts, picked, onpick }: Props = $props();

  let spectrum = $state.raw<Spectrum | null>(null);
  let error = $state("");
  let asked = 0;
  $effect(() => {
    const index = picked;
    const ask = ++asked;
    error = "";
    void loadWindow(target, index).then((o) => {
      if (ask !== asked) return;
      if (o.ok) spectrum = o.data;
      else error = o.error;
    });
  });

  /** across the axis from its lowest frequency (0) to its highest (1), as the image runs */
  const across = (hz: number) => 1 - heightOf(layout, hz);
  const points = $derived(
    spectrum ? spectrum.hz.map((hz, i) => ({ x: across(hz), db: spectrum!.db[i] })) : []
  );
  const ticks = $derived(
    (layout.scale === "log"
      ? [100, 1_000, 10_000]
      : [0, 5_000, 10_000, 15_000, 20_000, 40_000]
    ).filter((hz) => hz >= layout.minHz && hz <= layout.maxHz)
  );
  const label = (x: number) => {
    const hz = ticks.find((t) => Math.abs(across(t) - x) < 1e-9) ?? 0;
    return hz >= 1000 ? `${hz / 1000} kHz` : `${hz} Hz`;
  };
</script>

<div class="flex flex-col gap-2">
  <div class="flex items-center gap-2">
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label="Previous window"
      disabled={picked <= 0}
      onclick={() => onpick(picked - 1)}><ChevronLeftIcon /></Button>
    <p class="text-sm">
      Spectrum at <span class="tabular-nums">{formatDuration(starts[picked] ?? 0)}</span>
    </p>
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label="Next window"
      disabled={picked >= starts.length - 1}
      onclick={() => onpick(picked + 1)}><ChevronRightIcon /></Button>
  </div>
  {#if error}
    <p class="text-sm text-fg-muted">{error}</p>
  {:else if points.length}
    <div
      class="h-40"
      role="img"
      aria-label={`The spectrum of the window at ${formatDuration(starts[picked] ?? 0)}`}>
      <Chart
        data={points}
        x="x"
        y="db"
        xDomain={[0, 1]}
        yDomain={[-160, 0]}
        padding={{ left: 40, bottom: 20, top: 4, right: 8 }}>
        <Svg>
          <Grid y class="stroke-border" style="stroke-dasharray: 3 3" />
          <Axis
            placement="left"
            ticks={[-150, -100, -50, 0]}
            format={(db: number) => `${db} dB`}
            tickLabelProps={tickLabel} />
          <Axis
            placement="bottom"
            ticks={ticks.map(across)}
            format={label}
            tickLabelProps={tickLabel} />
          <Spline class="stroke-accent" strokeWidth={1.5} />
        </Svg>
      </Chart>
    </div>
  {/if}
</div>
