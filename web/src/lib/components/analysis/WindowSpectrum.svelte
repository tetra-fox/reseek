<script lang="ts">
  import { Axis, Chart, Grid, Spline, Svg } from "layerchart";

  import type { Scale } from "#lib/analysis/analysis.js";
  import { binHz, formatHz, frequencyTicks, heightOf } from "#lib/analysis/frequency.js";
  import type { Scope } from "#lib/analysis/scope.svelte.js";
  import { formatTime } from "#lib/analysis/timeview.js";
  import { tickLabel } from "#lib/dashboard/chart.js";

  // the spectrum of the window the cursor is in on LayerChart: level against frequency, on the
  // frequency axis the spectrogram uses. Every window's spectrum loads with the spectrogram, so
  // it follows the cursor as it is dragged
  type Props = { scope: Scope; scale: Scale };

  let { scope, scale }: Props = $props();

  const spectra = $derived(scope.pictures.spectra);
  const index = $derived(scope.window);
  const maxHz = $derived(scope.layout.maxHz);
  /** across the axis from its lowest frequency (0) to its highest (1), as the spectrogram runs */
  const across = (hz: number) => 1 - heightOf(scale, maxHz, hz);
  const points = $derived.by(() => {
    if (!spectra || index < 0 || index >= spectra.windows) return [];
    const { bins, minDb, maxDb, bytes } = spectra;
    const row = bytes.subarray(index * bins, (index + 1) * bins);
    return Array.from(row, (byte, bin) => ({
      x: across(binHz(bin, bins, maxHz)),
      db: minDb + (byte / 255) * (maxDb - minDb)
    }));
  });
  const ticks = $derived(
    frequencyTicks(scale, maxHz).filter((_, i, all) => all.length < 6 || i % 2 === 0)
  );
  const label = (x: number) => formatHz(ticks.find((t) => Math.abs(across(t) - x) < 1e-9) ?? 0);
  const at = $derived(index >= 0 ? formatTime(scope.starts[index] ?? 0) : "");
</script>

<div class="flex min-w-0 flex-col gap-1">
  <p class="text-sm">
    Spectrum at <span class="tabular-nums">{at}</span>
  </p>
  {#if points.length}
    <div class="h-40" role="img" aria-label={`The spectrum of the window at ${at}`}>
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
  {:else if spectra === null && !scope.pictures.lost && !scope.pictures.error}
    <p class="text-sm text-fg-muted">Loading...</p>
  {:else if spectra === null}
    <p class="text-sm text-fg-muted">The windows' spectra couldn't be loaded.</p>
  {/if}
</div>
