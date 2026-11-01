<script lang="ts">
  import { onMount } from "svelte";

  import { drawOverview, Rasters } from "#lib/analysis/spectrogram/draw.js";
  import { type Palette, readPalette } from "#lib/analysis/spectrogram/paint.js";
  import type { ScaleMotion } from "#lib/analysis/spectrogram/scalemotion.svelte.js";
  import type { Scope } from "#lib/analysis/spectrogram/scope.svelte.js";
  import { acrossOf, whole } from "#lib/analysis/spectrogram/timeview.js";

  // the whole file's spectrogram in a strip, the part in view outlined and the rest dimmed, with
  // the cursor; dragging the outline pans the view, and pressing elsewhere centers it there
  type Props = { scope: Scope; motion: ScaleMotion };

  let { scope, motion }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let width = $state(0);
  let height = $state(0);
  let dpr = $state(1);
  let palette = $state.raw<Palette | null>(null);
  const rasters = new Rasters();
  /** the canvas's corner radius in CSS pixels, which the outline of the view follows */
  let radius = 0;

  onMount(() => {
    dpr = window.devicePixelRatio || 1;
    if (!canvas) return;
    palette = readPalette(canvas);
    radius = parseFloat(getComputedStyle(canvas).borderTopLeftRadius);
  });

  let frame = 0;
  const draw = (now: number) => {
    frame = 0;
    const context = canvas?.getContext("2d");
    const overview = scope.pictures.spectrogram;
    if (!context || !palette || !overview) return;
    // a moving axis draws again next frame
    if (motion.moving(now)) frame = requestAnimationFrame(draw);
    const w = Math.round(width * dpr);
    const h = Math.round(height * dpr);
    context.clearRect(0, 0, w, h);
    const all = whole(scope.duration);
    const rect = { x: 0, y: 0, width: w, height: h };
    const curve = motion.at(now);
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
    const from = acrossOf(all, scope.view.start) * w;
    const to = acrossOf(all, scope.view.start + scope.view.span) * w;
    // the line is inset by half its width, so its middle takes the canvas's radius less that half
    // and its outer edge meets the rounded corners when the whole file is in view
    const outline = new Path2D();
    outline.roundRect(
      from + dpr / 2,
      dpr / 2,
      Math.max(dpr, to - from - dpr),
      h - dpr,
      Math.max(0, (radius - 0.5) * dpr)
    );
    const outside = new Path2D(outline);
    outside.rect(0, 0, w, h);
    context.fillStyle = palette.plot;
    context.globalAlpha = 0.6;
    context.fill(outside, "evenodd");
    context.globalAlpha = 1;
    context.strokeStyle = palette.fg;
    context.lineWidth = dpr;
    context.stroke(outline);
    context.fillStyle = palette.cursor;
    context.fillRect(Math.round(acrossOf(all, scope.cursor) * w), 0, dpr, h);
  };
  $effect(() => {
    void [
      width,
      height,
      dpr,
      motion.target,
      palette,
      scope.view,
      scope.cursor,
      scope.pictures.version
    ];
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
