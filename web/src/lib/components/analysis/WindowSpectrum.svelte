<script lang="ts">
  import { Axis, Chart, Grid, Spline, Svg } from "layerchart";
  import { onMount } from "svelte";

  import {
    axisTicks,
    binHz,
    type Curve,
    formatHz,
    heightOn
  } from "#lib/analysis/spectrogram/frequency.js";
  import type { Scope } from "#lib/analysis/spectrogram/scope.svelte.js";
  import { formatTime } from "#lib/analysis/spectrogram/timeview.js";
  import { chartCurve, tickLabel } from "#lib/dashboard/chart.js";

  // the spectrum of the window the cursor is in on LayerChart: level against frequency, on the
  // frequency axis the spectrogram uses, moving with it. Every window's spectrum loads with the
  // spectrogram, so it follows the cursor as it is dragged
  type Props = { scope: Scope; curve: Curve };

  let { scope, curve }: Props = $props();

  const PADDING = { left: 40, bottom: 20, top: 4, right: 8 };
  /** how many pixels of the axis one plotted point stands for */
  const SMOOTH_PX = 3;
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
  const inner = $derived(Math.max(0, width - PADDING.left - PADDING.right));
  /**
   * The window's levels as the line draws them: bins that fall within the same few pixels on
   * the current axis are averaged by power into one point, so the crowded top of a log or mel
   * axis reads as a curve rather than a comb of single-bin spikes
   */
  const points = $derived.by(() => {
    if (!spectra || index < 0 || index >= spectra.windows) return [];
    const { bins, minDb, maxDb, bytes } = spectra;
    const row = bytes.subarray(index * bins, (index + 1) * bins);
    const buckets = Math.max(1, Math.round(inner / SMOOTH_PX));
    const power = new Float64Array(buckets);
    const xs = new Float64Array(buckets);
    const count = new Uint32Array(buckets);
    row.forEach((byte, bin) => {
      const x = across(binHz(bin, bins, maxHz));
      const b = Math.min(buckets - 1, Math.max(0, Math.floor(x * buckets)));
      power[b] += 10 ** ((minDb + (byte / 255) * (maxDb - minDb)) / 10);
      xs[b] += x;
      count[b] += 1;
    });
    const out: { x: number; db: number }[] = [];
    for (let b = 0; b < buckets; b++) {
      if (!count[b]) continue;
      out.push({ x: xs[b] / count[b], db: Math.max(minDb, 10 * Math.log10(power[b] / count[b])) });
    }
    return out;
  });
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
          <Spline class="stroke-accent" strokeWidth={1.5} curve={chartCurve} />
        </Svg>
      </Chart>
    </div>
  {:else if spectra === null && !scope.pictures.lost && !scope.pictures.error}
    <p class="text-sm text-fg-muted">Loading...</p>
  {:else if spectra === null}
    <p class="text-sm text-fg-muted">Could not load the spectrum</p>
  {/if}
</div>
