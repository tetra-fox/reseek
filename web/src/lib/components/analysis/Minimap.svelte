<script lang="ts">
  import { onMount } from "svelte";

  import type { Scale } from "#lib/analysis/analysis.js";
  import { Rasters } from "#lib/analysis/draw.js";
  import {
    drawRaster,
    levelColors,
    loudest,
    type Palette,
    readPalette
  } from "#lib/analysis/paint.js";
  import type { Scope } from "#lib/analysis/scope.svelte.js";
  import { acrossOf, whole } from "#lib/analysis/timeview.js";

  // the whole file's spectrogram in a strip, the part in view outlined and the rest dimmed, with
  // the cursor; dragging the outline pans the view, and pressing elsewhere centers it there
  type Props = { scope: Scope; scale: Scale };

  let { scope, scale }: Props = $props();

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

  let frame = 0;
  const draw = () => {
    frame = 0;
    const context = canvas?.getContext("2d");
    const overview = scope.pictures.spectrogram;
    if (!context || !palette || !overview) return;
    const w = Math.round(width * dpr);
    const h = Math.round(height * dpr);
    context.clearRect(0, 0, w, h);
    const top = loudest(overview);
    const dbPerByte = ((overview.maxDb ?? 0) - (overview.minDb ?? -160)) / 255;
    const colors = () => levelColors(palette!.levels, top, dbPerByte);
    const raster = rasters.of(overview, scale, scope.layout.maxHz, h, colors, top);
    const all = whole(scope.duration);
    drawRaster(context, raster, overview, all, { x: 0, y: 0, width: w, height: h });
    const from = acrossOf(all, scope.view.start) * w;
    const to = acrossOf(all, scope.view.start + scope.view.span) * w;
    context.fillStyle = palette.plot;
    context.globalAlpha = 0.6;
    context.fillRect(0, 0, from, h);
    context.fillRect(to, 0, w - to, h);
    context.globalAlpha = 1;
    context.strokeStyle = palette.fg;
    context.lineWidth = dpr;
    context.strokeRect(from + dpr / 2, dpr / 2, Math.max(dpr, to - from - dpr), h - dpr);
    context.fillStyle = palette.cursor;
    context.fillRect(Math.round(acrossOf(all, scope.cursor) * w), 0, dpr, h);
  };
  $effect(() => {
    void [width, height, dpr, scale, palette, scope.view, scope.cursor, scope.pictures.version];
    if (!frame) frame = requestAnimationFrame(draw);
  });
  $effect(() => () => cancelAnimationFrame(frame));

  /** the view's start when the drag began and the time under the pointer then */
  let drag: { start: number; t: number } | null = null;
  const timeOf = (e: PointerEvent) => {
    const box = canvas!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)) * scope.duration;
  };
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    canvas!.setPointerCapture(e.pointerId);
    const t = timeOf(e);
    const { start, span } = scope.view;
    if (t < start || t > start + span) scope.centerOn(t);
    drag = { start: scope.view.start, t };
  };
  const move = (e: PointerEvent) => {
    if (!drag) return;
    scope.show({ start: drag.start + timeOf(e) - drag.t, span: scope.view.span });
    scope.following = false;
  };
</script>

<div class="h-6 w-full" bind:clientWidth={width} bind:clientHeight={height}>
  <canvas
    bind:this={canvas}
    class="block size-full cursor-grab touch-none rounded-control bg-plot active:cursor-grabbing"
    width={Math.round(width * dpr)}
    height={Math.round(height * dpr)}
    aria-hidden="true"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={() => (drag = null)}
    onpointercancel={() => (drag = null)}></canvas>
</div>
