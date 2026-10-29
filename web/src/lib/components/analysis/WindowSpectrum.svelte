<script lang="ts">
  import { Axis, Chart, Grid, Spline, Svg } from "layerchart";
  import { onMount } from "svelte";

  import { axisTicks, binHz, type Curve, formatHz, heightOn } from "#lib/analysis/frequency.js";
  import type { Scope } from "#lib/analysis/scope.svelte.js";
  import { formatTime } from "#lib/analysis/timeview.js";
  import { tickLabel } from "#lib/dashboard/chart.js";

  // the spectrum of the window the cursor is in on LayerChart: level against frequency, on the
  // frequency axis the spectrogram uses, moving with it. Every window's spectrum loads with the
  // spectrogram, so it follows the cursor as it is dragged
  type Props = { scope: Scope; curve: Curve };

  let { scope, curve }: Props = $props();

  const PADDING = { left: 40, bottom: 20, top: 4, right: 8 };
  let width = $state(0);
  let height = $state(0);
  let rem = $state(16);
  onMount(() => {
    rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  });

  const spectra = $derived(scope.pictures.spectra);
  const index = $derived(scope.window);
  const maxHz = $derived(scope.layout.maxHz);
  /** across the axis from its lowest frequency (0) to its highest (1), as the spectrogram runs */
  const across = (hz: number) => 1 - heightOn(curve, maxHz, hz);
  const points = $derived.by(() => {
    if (!spectra || index < 0 || index >= spectra.windows) return [];
    const { bins, minDb, maxDb, bytes } = spectra;
    const row = bytes.subarray(index * bins, (index + 1) * bins);
    return Array.from(row, (byte, bin) => ({
      x: across(binHz(bin, bins, maxHz)),
      db: minDb + (byte / 255) * (maxDb - minDb)
    }));
  });
  const inner = $derived(Math.max(0, width - PADDING.left - PADDING.right));
  // a label is up to two and a quarter rem wide
  const ticks = $derived(axisTicks(curve, maxHz, inner, 2.75 * rem));
  const at = $derived(index >= 0 ? formatTime(scope.starts[index] ?? 0) : "");
</script>

<div class="flex min-w-0 flex-col gap-1">
  <p class="text-sm">
    Spectrum at <span class="tabular-nums">{at}</span>
  </p>
  {#if points.length}
    <div
      class="h-40"
      role="img"
      aria-label={`The spectrum of the window at ${at}`}
      bind:clientWidth={width}
      bind:clientHeight={height}>
      <Chart data={points} x="x" y="db" xDomain={[0, 1]} yDomain={[-160, 0]} padding={PADDING}>
        <Svg>
          <Grid y class="stroke-border" style="stroke-dasharray: 3 3" />
          <Axis
            placement="left"
            ticks={[-150, -100, -50, 0]}
            format={(db: number) => `${db} dB`}
            tickLabelProps={tickLabel} />
          <!-- the frequency labels, drawn here so each keeps its element as it moves and fades -->
          {#each ticks as tick (tick.hz)}
            <g
              transform={`translate(${(1 - tick.at) * inner} ${height - PADDING.top - PADDING.bottom})`}
              opacity={tick.alpha}>
              <line y2="4" class="stroke-border" />
              <text
                y="7"
                text-anchor="middle"
                dominant-baseline="hanging"
                class={tickLabel.class}
                style={tickLabel.style}>{formatHz(tick.hz)}</text>
            </g>
          {/each}
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
