<script lang="ts" module>
  /** A series drawn as an area: at each point across the plot, its top and the floor it fills to */
  export type AreaFill = {
    key: string;
    color: string;
    x: number[];
    top: number[];
    floor: number[];
  };

  /** A bar, or a part of a stacked one, and the group of bars it is in, which a hover lifts */
  export type BarFill = {
    color: string;
    x0: number;
    x1: number;
    top: number;
    floor: number;
    group: number;
  };
</script>

<script lang="ts">
  import { prefersReducedMotion } from "svelte/motion";

  import { themeRgb } from "#lib/charts/theme.svelte.js";
  import {
    backingSize,
    bloomLayerStyle,
    clamp01,
    easeInOutCubic,
    easeOutCubic,
    paintColumn
  } from "#lib/vendor/dither-kit/dither-paint.js";
  import { rgb, type Seed } from "#lib/vendor/dither-kit/palette.js";

  // a chart's fills painted with dither-kit's ordered dither on a coarse canvas scaled up without
  // smoothing, over the plot of a LayerChart chart, whose axes and tooltip are drawn above it.
  // Areas sweep in from the left and bars grow in a wave when the chart first shows; a hover lifts
  // the fills and lays a blurred glow of them on top. Reduced motion shows them at once and still
  type Props = {
    /** the plot's size in pixels */
    width: number;
    height: number;
    areas?: AreaFill[];
    bars?: BarFill[];
    /** the pointer is over the chart */
    hovered?: boolean;
    /** where across the plot the hovered point is, marked down to the floor */
    marker?: number | null;
    /** the bars' group under the pointer */
    group?: number | null;
    /** the series the others dim beside */
    emphasis?: string | null;
    /** plays the entrance; a sparkline in a row shows at once */
    animate?: boolean;
    /** glows on hover */
    bloom?: boolean;
  };

  let {
    width,
    height,
    areas = [],
    bars = [],
    hovered = false,
    marker = null,
    group = null,
    emphasis = null,
    animate = true,
    bloom = true
  }: Props = $props();

  const ENTRANCE_MS = 900;
  /** the share of the entrance spent starting the bars one after another */
  const STAGGER = 0.55;

  let canvas = $state<HTMLCanvasElement | null>(null);
  let glow = $state<HTMLCanvasElement | null>(null);

  const size = $derived(backingSize(width, height));
  const seeds = $derived(new Map([...areas, ...bars].map((f) => [f.color, seedOf(f.color)])));
  const still = $derived(prefersReducedMotion.current || !animate);
  const glowStyle = $derived(bloom ? bloomLayerStyle("low", hovered) : null);

  /** A colour as a seed: dither-kit paints every pixel in the fill colour and varies its alpha */
  const seedOf = (color: string): Seed => {
    const fill = themeRgb(color);
    return { fill, line: fill, star: fill };
  };

  /** The value of `ys` at `x` along the points `xs`, straight between them */
  const along = (xs: number[], ys: number[], x: number) => {
    if (!xs.length) return 0;
    if (x <= xs[0]) return ys[0];
    for (let i = 1; i < xs.length; i++) {
      if (x <= xs[i]) {
        const t = (x - xs[i - 1]) / (xs[i] - xs[i - 1] || 1);
        return ys[i - 1] + (ys[i] - ys[i - 1]) * t;
      }
    }
    return ys.at(-1)!;
  };

  /** Each area's top and floor per canvas column, in canvas rows */
  const surfaces = $derived.by(() => {
    const { cols, rows } = size;
    const fy = (rows - 1) / Math.max(height, 1);
    return areas.map((a) => {
      const top: number[] = [];
      const floor: number[] = [];
      for (let c = 0; c < cols; c++) {
        const x = ((c + 0.5) / cols) * width;
        top.push(along(a.x, a.top, x) * fy);
        floor.push(along(a.x, a.floor, x) * fy);
      }
      return { key: a.key, color: a.color, top, floor };
    });
  });

  // the stars that wink in the areas, placed the same way every time; none in a still chart
  const stars = $derived(
    !animate
      ? []
      : surfaces.flatMap((s, k) =>
          Array.from({ length: Math.max(4, Math.round(size.cols / 14)) }, (_, i) => {
            const seed = i * 67 + 13 + k * 131;
            return {
              area: k,
              col: seed % Math.max(size.cols, 1),
              depth: ((seed * 53 + 7) % 100) / 100,
              phase: (seed * 41) % 360
            };
          })
        )
  );

  /** when the entrance started; it plays once, as the chart first shows */
  let since = 0;
  /** how lifted the fills are, easing toward 1 while hovered */
  let lift = 0;
  /** the fills, painted again only when they change, under the marker and the stars */
  const fills = typeof document === "undefined" ? null : document.createElement("canvas");
  /** what the fills were last painted from */
  let painted = "";
  /** when the stars last winked; they wink ten times a second */
  let winked = 0;
  /** what the canvas last showed */
  let shown = "";

  /** Paints the fills onto `fills` for the entrance's `progress` */
  const paintFills = (f: CanvasRenderingContext2D, progress: number) => {
    const { cols, rows } = size;
    f.clearRect(0, 0, cols, rows);
    const reveal = Math.ceil(easeInOutCubic(progress) * cols);
    surfaces.forEach((s, i) => {
      const seed = seeds.get(s.color)!;
      const dim = emphasis !== null && emphasis !== s.key ? 0.3 : 1;
      for (let x = 0; x < Math.min(cols, reveal + 1); x++) {
        const a = s.top[x];
        const b = s.floor[x];
        paintColumn(f, x, Math.min(a, b), Math.max(a, b), seed, {
          variant: "gradient",
          intensity: lift,
          dim,
          stacked: false,
          // layers in front thin out so each reads as its own
          sparse: i * 0.14
        });
      }
    });
    const fx = cols / Math.max(width, 1);
    const fy = (rows - 1) / Math.max(height, 1);
    const groups = Math.max(1, ...bars.map((b) => b.group + 1));
    for (const bar of bars) {
      const start = groups > 1 ? (bar.group / (groups - 1)) * STAGGER : 0;
      const grown = still ? 1 : easeOutCubic(clamp01((progress - start) / (1 - STAGGER)));
      const active = group === bar.group;
      const floor = bar.floor * fy;
      const top = floor + (bar.top * fy - floor) * grown;
      const seed = seeds.get(bar.color)!;
      for (let x = Math.round(bar.x0 * fx); x < Math.round(bar.x1 * fx); x++) {
        paintColumn(f, x, Math.min(top, floor), Math.max(top, floor), seed, {
          variant: "gradient",
          intensity: lift + (active ? 0.4 : 0),
          dim: group !== null && !active ? 0.5 : 1,
          stacked: true
        });
      }
    }
  };

  /** Draws a frame; whether another is needed */
  const paint = (now: number): boolean => {
    const c = canvas?.getContext("2d");
    const f = fills?.getContext("2d");
    if (!c || !f || !fills) return false;
    const { cols, rows } = size;
    if (!since) since = now;
    const progress = still ? 1 : clamp01((now - since) / ENTRANCE_MS);
    const target = hovered ? 1 : 0;
    lift = still ? target : lift + (target - lift) * 0.16;
    if (Math.abs(lift - target) < 0.001) lift = target;
    const key = `${cols}x${rows}|${progress}|${lift}|${emphasis}|${group}`;
    if (key !== painted || fills.width !== cols || fills.height !== rows) {
      fills.width = cols;
      fills.height = rows;
      paintFills(f, progress);
      painted = key;
    }
    const wink = !still && now - winked >= 100;
    if (wink) winked = now;
    const frame = `${painted}|${winked}|${marker}`;
    if (frame === shown) return true;
    shown = frame;
    c.clearRect(0, 0, cols, rows);
    c.drawImage(fills, 0, 0);

    // the hovered point: its column down to the floor and a block on each area's top
    const reveal = Math.ceil(easeInOutCubic(progress) * cols);
    const mx = marker === null ? -1 : Math.round((marker * cols) / Math.max(width, 1));
    if (mx >= 0 && mx < reveal) {
      for (const s of surfaces) {
        const fill = seeds.get(s.color)!.fill;
        const my = Math.round(s.top[mx]);
        c.fillStyle = rgb(fill, 1, 0.55);
        c.fillRect(mx, my, 1, Math.max(0, Math.round(s.floor[mx]) - my));
        c.fillStyle = rgb(fill);
        c.fillRect(mx - 1, my - 1, 3, 3);
      }
    }

    // stars glint in the area's colour
    for (const star of stars) {
      const s = surfaces[star.area];
      if (!s || star.col >= reveal) continue;
      const y = Math.round(s.top[star.col] + star.depth * (s.floor[star.col] - s.top[star.col]));
      const twinkle = (Math.sin(winked / 285 + star.phase) + 1) / 2;
      const alpha = twinkle * (0.7 + 0.3 * lift);
      if (alpha < 0.55 || y < 0 || y >= rows) continue;
      c.fillStyle = rgb(seeds.get(s.color)!.fill, 1, alpha);
      c.fillRect(star.col, y, 1, 1);
    }

    const g = glow?.getContext("2d");
    if (g && glowStyle) {
      g.clearRect(0, 0, cols, rows);
      g.drawImage(canvas!, 0, 0);
    }
    // goes on while something moves: the entrance, the lift, the stars
    return progress < 1 || lift !== target || (!still && stars.length > 0);
  };

  // repaints when what is drawn changes, and every frame while it moves
  $effect(() => {
    void [surfaces, bars, hovered, marker, group, emphasis, seeds, glowStyle, size];
    if (!canvas) return;
    painted = "";
    shown = "";
    canvas.width = size.cols;
    canvas.height = size.rows;
    if (glow) {
      glow.width = size.cols;
      glow.height = size.rows;
    }
    let frame = requestAnimationFrame(function step(now) {
      if (paint(now)) frame = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

<canvas
  bind:this={canvas}
  class="pointer-events-none absolute inset-0 [image-rendering:pixelated]"
  style:width={`${width}px`}
  style:height={`${height}px`}
  aria-hidden="true"></canvas>
{#if bloom}
  <canvas
    bind:this={glow}
    class={[
      "pointer-events-none absolute inset-0",
      !prefersReducedMotion.current && "transition-opacity duration-200"
    ]}
    style:width={`${width}px`}
    style:height={`${height}px`}
    style:filter={glowStyle?.filter}
    style:opacity={glowStyle?.opacity ?? 0}
    style:mix-blend-mode={glowStyle?.mixBlendMode}
    aria-hidden="true"></canvas>
{/if}
