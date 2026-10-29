<script lang="ts">
  import { onMount } from "svelte";

  import {
    acrossRect,
    drawTimeline,
    inside,
    Rasters,
    type Stage,
    stageOf,
    wantsDetail
  } from "#lib/analysis/spectrogram/draw.js";
  import { type Palette, readPalette } from "#lib/analysis/spectrogram/paint.js";
  import type { ScaleMotion } from "#lib/analysis/spectrogram/scalemotion.svelte.js";
  import type { Scope } from "#lib/analysis/spectrogram/scope.svelte.js";
  import { acrossOf, formatTime, showsAll, timeAt } from "#lib/analysis/spectrogram/timeview.js";

  // the spectrogram over the waveform on one time axis, drawn on one canvas at device pixels,
  // with the cursor across both. Pressing on the cursor and dragging moves it; pressing
  // elsewhere and dragging pans a zoomed view (or moves the cursor when the whole file is in
  // view); a click puts the cursor there. The wheel with Ctrl, or a pinch, zooms around the
  // pointer, and the arrow keys step the cursor a window at a time
  type Props = {
    scope: Scope;
    /** the frequency axis, drawn where it is at each frame's time */
    motion: ScaleMotion;
    /** the large view: taller plots, and the plain wheel zooms as nothing scrolls under it */
    large?: boolean;
    /** what the cursor reads as, for assistive technology */
    readout: string;
    /** the cursor was put somewhere by the user, for playback to follow */
    onseek: (t: number) => void;
    /** play or pause, from the space bar */
    ontoggle?: () => void;
    /** the plain wheel was turned where it scrolls the page, so a hint can say how to zoom */
    onwheelhint?: () => void;
  };

  let { scope, motion, large = false, readout, onseek, ontoggle, onwheelhint }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  let width = $state(0);
  let rem = $state(16);
  let dpr = $state(1);
  let palette = $state.raw<Palette | null>(null);
  let hover = $state.raw<{ x: number; y: number } | null>(null);
  const rasters = new Rasters();

  const stage = $derived<Stage>(stageOf(width, dpr, rem, large ? 22 : 13, large ? 5 : 3.5));
  /** a plot's place in CSS pixels, written on the canvas for what drives the page from outside */
  const box = (rect: Stage["waveform"]) =>
    [rect.x, rect.y, rect.width, rect.height].map((v) => (v / dpr).toFixed(1)).join(" ");

  onMount(() => {
    rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
    dpr = window.devicePixelRatio || 1;
    if (canvas) palette = readPalette(canvas);
    // a page zoomed or moved to another screen changes its pixel ratio
    const ratio = matchMedia(`(resolution: ${dpr}dppx)`);
    const changed = () => (dpr = window.devicePixelRatio || 1);
    ratio.addEventListener("change", changed);
    return () => ratio.removeEventListener("change", changed);
  });

  let frame = 0;
  const draw = (now: number) => {
    frame = 0;
    const context = canvas?.getContext("2d");
    if (!context || !palette || !width) return;
    // a moving axis draws again next frame
    if (motion.moving(now)) frame = requestAnimationFrame(draw);
    drawTimeline(
      context,
      stage,
      {
        pictures: scope.pictures,
        view: scope.view,
        curve: motion.at(now),
        cursor: scope.cursor,
        hidden: scope.hidden,
        palette,
        hover
      },
      rasters
    );
  };
  // everything drawn is read here, so any change draws again, once a frame
  $effect(() => {
    void [stage, motion.target, palette, hover, scope.view, scope.cursor, scope.pictures.version];
    void [...scope.hidden];
    if (!frame) frame = requestAnimationFrame(draw);
  });
  $effect(() => () => cancelAnimationFrame(frame));

  // tiles at full detail for what is in view and half a view either side, when it shows more
  // than the overview holds
  $effect(() => {
    const { pictures, view } = scope;
    const overview = pictures.spectrogram;
    if (!overview || !width) return;
    const { columnsPerSecond, durationSeconds } = pictures.layout;
    if (!wantsDetail(overview, view, stage.spectrogram.width, columnsPerSecond, durationSeconds)) {
      return;
    }
    pictures.forget();
    pictures.want(view.start - view.span / 2, view.start + view.span * 1.5);
  });

  /** A pointer's place on the canvas in device pixels */
  const at = (e: PointerEvent | WheelEvent) => {
    const box = canvas!.getBoundingClientRect();
    return { x: (e.clientX - box.left) * dpr, y: (e.clientY - box.top) * dpr };
  };
  const plots = (x: number, y: number) =>
    inside(stage.spectrogram, x, y) || inside(stage.waveform, x, y);
  /** The time at a point, kept inside the view */
  const timeOf = (x: number) =>
    timeAt(scope.view, Math.min(1, Math.max(0, acrossRect(stage.spectrogram, x))));

  /** What a pressed pointer is doing */
  type Press =
    | { kind: "pending"; x: number; y: number; start: typeof scope.view }
    | { kind: "cursor" }
    | { kind: "pan"; x: number; start: typeof scope.view };
  let press: Press | null = null;
  /**
   * The mouse cursor over the canvas: arrows both ways on the cursor line, which drags; a hand
   * over a zoomed plot, which pans; the arrow elsewhere
   */
  let pointerStyle = $state("default");
  const nearCursor = (x: number) =>
    Math.abs(
      x - (stage.spectrogram.x + acrossOf(scope.view, scope.cursor) * stage.spectrogram.width)
    ) <=
    0.5 * stage.rem;
  const styleAt = (p: { x: number; y: number }) => {
    if (press?.kind === "cursor") return "ew-resize";
    if (press?.kind === "pan") return "grabbing";
    if (!plots(p.x, p.y)) return "default";
    if (nearCursor(p.x)) return "ew-resize";
    return showsAll(scope.view, scope.duration) ? "default" : "grab";
  };
  /** the pointers down, for a pinch */
  const pointers = new Map<number, { x: number; y: number }>();
  let pinch: { distance: number; span: number } | null = null;
  /** a cursor move waiting for the next frame, so a fast pointer moves it once a frame */
  let pending: number | null = null;
  let moveFrame = 0;
  const moveCursor = (t: number) => {
    pending = t;
    if (moveFrame) return;
    moveFrame = requestAnimationFrame(() => {
      moveFrame = 0;
      if (pending !== null) scope.drag(pending);
      pending = null;
    });
  };

  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    const p = at(e);
    pointers.set(e.pointerId, p);
    canvas!.setPointerCapture(e.pointerId);
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      pinch = { distance: Math.hypot(a.x - b.x, a.y - b.y), span: scope.view.span };
      press = null;
      return;
    }
    if (!plots(p.x, p.y)) return;
    // the line is easy to grab: half a rem either side of it
    press = nearCursor(p.x)
      ? { kind: "cursor" }
      : { kind: "pending", x: p.x, y: p.y, start: scope.view };
    scope.dragging = press.kind === "cursor";
    pointerStyle = styleAt(p);
  };

  const move = (e: PointerEvent) => {
    const p = at(e);
    hover = plots(p.x, p.y) ? p : null;
    pointerStyle = styleAt(p);
    if (!pointers.has(e.pointerId)) return;
    pointers.set(e.pointerId, p);
    if (pinch && pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      const distance = Math.hypot(a.x - b.x, a.y - b.y);
      const middle = acrossRect(stage.spectrogram, (a.x + b.x) / 2);
      scope.zoom((scope.view.span / pinch.span) * (distance / pinch.distance), middle, false);
      return;
    }
    if (!press) return;
    if (press.kind === "pending" && Math.abs(p.x - press.x) > 0.25 * stage.rem) {
      press = showsAll(scope.view, scope.duration)
        ? { kind: "cursor" }
        : { kind: "pan", x: press.x, start: press.start };
      scope.dragging = press.kind === "cursor";
    }
    if (press.kind === "cursor") moveCursor(timeOf(p.x));
    if (press.kind === "pan") {
      const seconds = ((press.x - p.x) / stage.spectrogram.width) * press.start.span;
      scope.show({ start: press.start.start + seconds, span: press.start.span });
      scope.following = false;
    }
  };

  const up = (e: PointerEvent) => {
    pointers.delete(e.pointerId);
    if (pointers.size < 2) pinch = null;
    if (!press) return;
    const p = at(e);
    if (press.kind === "pending") {
      scope.place(timeOf(p.x));
      onseek(scope.cursor);
    } else if (press.kind === "cursor") {
      if (pending !== null) scope.drag(pending);
      pending = null;
      onseek(scope.cursor);
    }
    press = null;
    scope.dragging = false;
    pointerStyle = styleAt(p);
  };

  const wheel = (e: WheelEvent) => {
    const p = at(e);
    if (!plots(p.x, p.y)) return;
    const lines = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
    const sideways = e.shiftKey ? e.deltaY : e.deltaX;
    if (Math.abs(sideways) > Math.abs(e.shiftKey ? 0 : e.deltaY) && !e.ctrlKey) {
      e.preventDefault();
      scope.pan(((sideways * lines) / (stage.spectrogram.width / dpr)) * scope.view.span);
      return;
    }
    // a pinch on a touchpad arrives as the wheel with Ctrl
    if (!e.ctrlKey && !e.metaKey && !large) {
      onwheelhint?.();
      return;
    }
    e.preventDefault();
    scope.zoom(Math.exp(-e.deltaY * lines * 0.002), acrossRect(stage.spectrogram, p.x));
  };

  // added by hand, as a listener that stops the page scrolling must not be passive
  $effect(() => {
    if (!canvas) return;
    const target = canvas;
    target.addEventListener("wheel", wheel, { passive: false });
    return () => target.removeEventListener("wheel", wheel);
  });

  const key = (e: KeyboardEvent) => {
    const steps = e.shiftKey ? 10 : 1;
    const run: Record<string, () => void> = {
      "ArrowLeft": () => scope.step(-steps),
      "ArrowRight": () => scope.step(steps),
      "Home": () => scope.toEnd(false),
      "End": () => scope.toEnd(true),
      "+": () => scope.zoomAtCursor(2),
      "=": () => scope.zoomAtCursor(2),
      "-": () => scope.zoomAtCursor(0.5),
      " ": () => ontoggle?.()
    };
    const action = run[e.key];
    if (!action || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    action();
    if (e.key.startsWith("Arrow") || e.key === "Home" || e.key === "End") onseek(scope.cursor);
  };
</script>

<div class="w-full" bind:clientWidth={width}>
  <canvas
    bind:this={canvas}
    class="block w-full touch-pan-y rounded-control bg-plot focus-visible:outline-2 focus-visible:outline-accent"
    style:height={`${stage.height / dpr}px`}
    style:cursor={pointerStyle}
    width={stage.width}
    height={stage.height}
    tabindex="0"
    role="slider"
    aria-label="Cursor"
    aria-valuemin={0}
    aria-valuemax={Math.round(scope.duration * 10) / 10}
    aria-valuenow={Math.round(scope.cursor * 10) / 10}
    aria-valuetext={readout}
    data-view={`${scope.view.start.toFixed(3)} ${scope.view.span.toFixed(3)}`}
    data-plot={box(stage.spectrogram)}
    data-wave={box(stage.waveform)}
    title={`Cursor at ${formatTime(scope.cursor, true)}`}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onpointerleave={() => (hover = null)}
    onkeydown={key}></canvas>
</div>
