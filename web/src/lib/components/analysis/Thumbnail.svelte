<script lang="ts">
  import { onMount } from "svelte";

  import { drawOverview, Rasters } from "#lib/analysis/spectrogram/draw.js";
  import { pure } from "#lib/analysis/spectrogram/frequency.js";
  import { type Palette, readPalette } from "#lib/analysis/spectrogram/paint.js";
  import type { Scope } from "#lib/analysis/spectrogram/scope.svelte.js";
  import { spectrogramView } from "#lib/analysis/spectrogram/view.svelte.js";

  // the whole file's spectrogram, small, which opens the large view. The large view's button says
  // the same to assistive technology, so this is left out of the tab order
  type Props = { scope: Scope; onopen: () => void };

  let { scope, onopen }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let width = $state(0);
  let height = $state(0);
  let dpr = $state(1);
  let palette = $state.raw<Palette | null>(null);
  const rasters = new Rasters();

  onMount(() => {
    dpr = window.devicePixelRatio || 1;
    if (canvas) palette = readPalette(canvas);
  });
  $effect(() => void scope.pictures.overview());

  const curve = $derived(pure(spectrogramView.scale ?? scope.layout.scale));
  $effect(() => {
    const context = canvas?.getContext("2d");
    const overview = scope.pictures.spectrogram;
    if (!context || !palette || !overview || !width || !height) return;
    const rect = { x: 0, y: 0, width: Math.round(width * dpr), height: Math.round(height * dpr) };
    context.clearRect(0, 0, rect.width, rect.height);
    drawOverview(
      context,
      rasters,
      overview,
      curve,
      scope.layout.maxHz,
      palette,
      scope.duration,
      rect
    );
  });
</script>

{#if scope.pictures.error}
  <p class="text-xs text-fg-muted">{scope.pictures.error}</p>
{:else}
  <button
    type="button"
    class="block h-20 w-full cursor-zoom-in"
    tabindex="-1"
    aria-hidden="true"
    bind:clientWidth={width}
    bind:clientHeight={height}
    onclick={onopen}>
    <canvas
      bind:this={canvas}
      class="block size-full rounded-control bg-plot"
      width={Math.round(width * dpr)}
      height={Math.round(height * dpr)}></canvas>
  </button>
{/if}
