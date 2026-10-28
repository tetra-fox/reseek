<script lang="ts">
  import { heightOf, type Layout, type Report } from "#lib/analysis/analysis.js";
  import { formatDuration } from "#lib/format.js";

  // every analysed window side by side, each bar as tall as where its content stops on the
  // spectrogram's frequency axis; near-silent windows have no bar. A click or the arrow keys
  // pick a window to see its spectrum
  type Props = {
    layout: Layout;
    windows: NonNullable<Report["windows"]>;
    picked: number | null;
    onpick: (index: number) => void;
  };

  let { layout, windows, picked, onpick }: Props = $props();

  /** where each window's content stops, null for a near-silent one */
  const cutoffs = $derived(windows.map((w) => w.cutoffHz ?? null));
  const bars = $derived(
    cutoffs.map((hz, i) => ({ i, top: hz === null ? null : heightOf(layout, hz) * 100 }))
  );
  const last = $derived(windows.length - 1);

  const describe = (i: number) => {
    const hz = cutoffs[i];
    const stops = hz === null ? "near silent" : `stops at ${(hz / 1000).toFixed(1)} kHz`;
    return `${formatDuration(windows[i].t)}, ${stops}`;
  };

  const pick = (e: MouseEvent) => {
    const box = (e.currentTarget as Element).getBoundingClientRect();
    const across = (e.clientX - box.left) / box.width;
    onpick(Math.min(last, Math.max(0, Math.floor(across * windows.length))));
  };
  const step = (e: KeyboardEvent) => {
    const to = {
      ArrowLeft: (picked ?? 0) - 1,
      ArrowRight: (picked ?? -1) + 1,
      Home: 0,
      End: last
    }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    onpick(Math.min(last, Math.max(0, to)));
  };
</script>

<div
  class="h-12 cursor-pointer rounded-control border bg-surface-muted focus-visible:outline-2 focus-visible:outline-accent"
  role="slider"
  tabindex="0"
  aria-label="Window"
  aria-valuemin={0}
  aria-valuemax={last}
  aria-valuenow={picked ?? 0}
  aria-valuetext={picked === null ? "None picked" : describe(picked)}
  onclick={pick}
  onkeydown={step}>
  <svg
    class="block size-full"
    viewBox={`0 0 ${windows.length} 100`}
    preserveAspectRatio="none"
    shape-rendering="crispEdges"
    aria-hidden="true">
    {#each bars as bar (bar.i)}
      {#if bar.top !== null}
        <rect
          x={bar.i}
          y={bar.top}
          width="1"
          height={100 - bar.top}
          class={bar.i === picked ? "fill-accent" : "fill-fg-muted/50"} />
      {/if}
    {/each}
    {#if picked !== null}
      <rect x={picked} y="0" width="1" height="100" class="fill-accent/25" />
    {/if}
  </svg>
</div>
