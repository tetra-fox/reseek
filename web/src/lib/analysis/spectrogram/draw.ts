import type { Guide, Scale } from "#lib/analysis/analysis.js";

import {
  axisTicks,
  type Curve,
  formatHz,
  heightOn,
  hzOn,
  pure,
  rowBins,
  scaleOf
} from "./frequency";
import {
  binRaster,
  drawRaster,
  levelAt,
  levelColors,
  loudest,
  type Palette,
  peakBetween,
  type Rect,
  warp
} from "./paint";
import type { Cells, Pictures } from "./pictures.svelte";
import { acrossOf, formatTime, timeAt, timeTicks, type View, whole } from "./timeview";

/** Where everything goes on the timeline's canvas, in device pixels */
export type Stage = {
  width: number;
  height: number;
  /** device pixels per CSS pixel */
  dpr: number;
  /** device pixels per rem, which text and gaps follow */
  rem: number;
  spectrogram: Rect;
  waveform: Rect;
  /** the time axis under the waveform */
  axis: Rect;
};

/** The share of the room for plots that goes to the waveform */
const WAVE_SHARE = 0.2;

/** The stage for a canvas `width` by `height` CSS pixels: the plots share what the axis leaves */
export const stageOf = (width: number, height: number, dpr: number, rem: number): Stage => {
  const r = rem * dpr;
  const left = Math.round(3.25 * r);
  const right = Math.round(0.5 * r);
  const inner = Math.max(1, Math.round(width * dpr) - left - right);
  const total = Math.round(height * dpr);
  const gap = Math.round(0.25 * r);
  const under = Math.round(1.4 * r);
  const room = Math.max(2, total - gap - under);
  const waveHeight = Math.max(1, Math.round(room * WAVE_SHARE));
  const spectrogram = { x: left, y: 0, width: inner, height: room - waveHeight };
  const waveform = { x: left, y: spectrogram.height + gap, width: inner, height: waveHeight };
  const axis = { x: left, y: waveform.y + waveHeight, width: inner, height: under };
  return {
    width: Math.round(width * dpr),
    height: total,
    dpr,
    rem: r,
    spectrogram,
    waveform,
    axis
  };
};

/** Where `x` (device pixels) falls across a rect, from 0 at its left edge to 1 at its right */
export const acrossRect = (rect: Rect, x: number) => (x - rect.x) / rect.width;

/** Whether the point is inside a rect */
export const inside = (rect: Rect, x: number, y: number) =>
  x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height;

/** Each encoder line's own color, so the legend tells them apart; all read on the dark plot */
const ENCODER = ["#5ad2ff", "#7cffb2", "#c3a6ff", "#ffffff", "#ffb86b"];

/** A guide line's color: the file limit red, the analyzer's cutoff yellow, encoders by turn */
export const guideColor = (guide: Guide, guides: Guide[]) =>
  guide.kind === "file_limit"
    ? "#ff7878"
    : guide.kind === "cutoff"
      ? "#ffd850"
      : ENCODER[guides.filter((g) => g.kind === "encoder").indexOf(guide) % ENCODER.length];

/** A guide's key, which the legend toggles it by */
export const guideKey = (guide: Guide) => `${guide.kind}:${guide.hz}`;

/** What a timeline draws */
export type Scene = {
  pictures: Pictures;
  view: View;
  /** the frequency axis, a scale or on its way between two */
  curve: Curve;
  cursor: number;
  hidden: ReadonlySet<string>;
  palette: Palette;
  /** where the pointer is over the canvas, in device pixels, null when it is elsewhere */
  hover: { x: number; y: number } | null;
};

/**
 * The images spectrograms are drawn from, kept for each set of columns: the bin raster, and the
 * image warped from it for the scale and height drawn
 */
export class Rasters {
  readonly #bins = new WeakMap<Cells, { top: number; canvas: HTMLCanvasElement }>();
  readonly #kept = new WeakMap<Cells, { key: string; canvas: HTMLCanvasElement }>();
  /** the rows of the axis last drawn between scales, made once a frame */
  #rows: { curve: Curve; key: string; table: Float32Array } | null = null;

  #binsOf(cells: Cells, colors: () => Uint8ClampedArray, top: number) {
    const kept = this.#bins.get(cells);
    if (kept?.top === top) return kept.canvas;
    const canvas = binRaster(cells, colors());
    this.#bins.set(cells, { top, canvas });
    return canvas;
  }

  #of(
    cells: Cells,
    scale: Scale,
    maxHz: number,
    height: number,
    colors: () => Uint8ClampedArray,
    top: number
  ) {
    const key = `${scale}:${height}:${top}`;
    const kept = this.#kept.get(cells);
    if (kept?.key === key) return kept.canvas;
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, cells.count);
    canvas.height = Math.max(1, height);
    const context = canvas.getContext("2d");
    if (context && cells.count) {
      const rows = rowBins(pure(scale), maxHz, cells.width, height);
      const rect = { x: 0, y: 0, width: cells.count, height };
      warp(context, this.#binsOf(cells, colors, top), rows, 0, cells.count, rect);
    }
    this.#kept.set(cells, { key, canvas });
    return canvas;
  }

  /**
   * Draws the part of `cells` in `view` into `rect` on `curve`: the image kept for a scale, or,
   * while the axis is between scales, the bin raster warped onto it
   */
  draw(
    context: CanvasRenderingContext2D,
    cells: Cells,
    curve: Curve,
    maxHz: number,
    colors: () => Uint8ClampedArray,
    top: number,
    view: View,
    rect: Rect
  ) {
    const scale = scaleOf(curve);
    if (scale) {
      const raster = this.#of(cells, scale, maxHz, rect.height, colors, top);
      drawRaster(context, raster, cells, view, rect);
      return;
    }
    const key = `${maxHz}:${cells.width}:${rect.height}`;
    if (this.#rows?.curve !== curve || this.#rows.key !== key) {
      this.#rows = { curve, key, table: rowBins(curve, maxHz, cells.width, rect.height) };
    }
    drawRaster(context, this.#binsOf(cells, colors, top), cells, view, rect, this.#rows.table);
  }
}

/** Draws the whole of a file's overview into `rect` on `curve`, in the palette's levels */
export const drawOverview = (
  context: CanvasRenderingContext2D,
  rasters: Rasters,
  overview: Cells,
  curve: Curve,
  maxHz: number,
  palette: Palette,
  duration: number,
  rect: Rect
) => {
  const top = loudest(overview);
  const dbPerByte = ((overview.maxDb ?? 0) - (overview.minDb ?? -160)) / 255;
  const colors = () => levelColors(palette.levels, top, dbPerByte);
  rasters.draw(context, overview, curve, maxHz, colors, top, whole(duration), rect);
};

/** How many full-detail cells of `perSecond` a file `duration` seconds long has */
const detailOf = (duration: number, perSecond: number) => Math.ceil(duration * perSecond - 1e-9);

/**
 * Whether a view `width` device pixels wide shows more of `overview` than it has cells for, and
 * full detail has more
 */
export const wantsDetail = (
  overview: Cells,
  view: View,
  width: number,
  perSecond: number,
  duration: number
) =>
  overview.count < detailOf(duration, perSecond) &&
  (overview.count * view.span) / (overview.to - overview.from) < width;

/** The tiles of `kind` that overlap `view`, those that have arrived */
const tilesIn = (pictures: Pictures, kind: "spectrogram" | "waveform", view: View): Cells[] =>
  pictures.tilesOver(kind, view.start, view.start + view.span);

/** The hatch each canvas fills unread stretches with, made once for its colors and size */
const hatches = new WeakMap<
  CanvasRenderingContext2D,
  { key: string; fill: CanvasPattern | string }
>();

/** The hatch unread stretches are filled with */
const hatch = (context: CanvasRenderingContext2D, palette: Palette, size: number) => {
  const key = `${palette.plot}:${palette.unread}:${size}`;
  const kept = hatches.get(context);
  if (kept?.key === key) return kept.fill;
  const tile = document.createElement("canvas");
  tile.width = tile.height = size;
  const t = tile.getContext("2d");
  if (t) {
    t.fillStyle = palette.plot;
    t.fillRect(0, 0, size, size);
    t.strokeStyle = palette.unread;
    t.lineWidth = size / 4;
    t.beginPath();
    t.moveTo(0, size);
    t.lineTo(size, 0);
    t.stroke();
  }
  const fill = context.createPattern(tile, "repeat") ?? palette.plot;
  hatches.set(context, { key, fill });
  return fill;
};

/** Draws the spectrogram, the waveform, their axes, the guide lines, the cursor and the hover readout */
export const drawTimeline = (
  context: CanvasRenderingContext2D,
  stage: Stage,
  scene: Scene,
  rasters: Rasters
) => {
  const { pictures, view, curve, palette } = scene;
  const { layout } = pictures;
  const { spectrogram: spec, waveform: wave, axis, rem, dpr } = stage;
  context.clearRect(0, 0, stage.width, stage.height);
  const unread = hatch(context, palette, Math.max(4, Math.round(0.5 * rem)));
  context.fillStyle = unread;
  context.fillRect(spec.x, spec.y, spec.width, spec.height);
  context.fillRect(wave.x, wave.y, wave.width, wave.height);
  const overview = pictures.spectrogram;
  if (!overview || view.span <= 0) return;

  // the spectrogram, the overview first and the tiles at full detail over it
  const top = loudest(overview);
  const dbPerByte = ((overview.maxDb ?? 0) - (overview.minDb ?? -160)) / 255;
  const colors = () => levelColors(palette.levels, top, dbPerByte);
  context.imageSmoothingEnabled = true;
  const paint = (cells: Cells) =>
    rasters.draw(context, cells, curve, layout.maxHz, colors, top, view, spec);
  paint(overview);
  const detail = wantsDetail(
    overview,
    view,
    spec.width,
    layout.columnsPerSecond,
    layout.durationSeconds
  );
  const shown = detail ? tilesIn(pictures, "spectrogram", view) : [];
  shown.forEach(paint);

  context.lineWidth = Math.max(1, Math.round(dpr));
  for (const guide of layout.guides) {
    if (scene.hidden.has(guideKey(guide))) continue;
    const y =
      Math.round(spec.y + heightOn(curve, layout.maxHz, guide.hz) * (spec.height - 1)) + 0.5;
    context.strokeStyle = guideColor(guide, layout.guides);
    context.setLineDash(guide.kind === "cutoff" ? [4 * dpr, 3 * dpr] : []);
    context.beginPath();
    context.moveTo(spec.x, y);
    context.lineTo(spec.x + spec.width, y);
    context.stroke();
  }
  context.setLineDash([]);

  // the frequency axis on the left, labels kept a line apart
  context.font = `${Math.round(0.7 * rem)}px ui-sans-serif, system-ui, sans-serif`;
  context.textAlign = "right";
  context.textBaseline = "middle";
  context.fillStyle = palette.muted;
  for (const { hz, at, alpha } of axisTicks(curve, layout.maxHz, spec.height, 0.9 * rem)) {
    const y = spec.y + at * spec.height;
    context.globalAlpha = alpha;
    context.fillRect(spec.x - 0.25 * rem, Math.round(y), 0.2 * rem, dpr);
    const label = Math.min(spec.y + spec.height - 0.4 * rem, Math.max(spec.y + 0.4 * rem, y));
    context.fillText(formatHz(hz), spec.x - 0.35 * rem, label);
  }
  context.globalAlpha = 1;

  // the waveform, from the tiles at full detail where they have arrived, else the overview
  const waves = pictures.waveform;
  if (waves) {
    const tiles = wantsDetail(
      waves,
      view,
      wave.width,
      layout.peaksPerSecond,
      layout.durationSeconds
    )
      ? tilesIn(pictures, "waveform", view)
      : [];
    const middle = wave.y + wave.height / 2;
    const half = wave.height / 2 - dpr;
    const peaks = Array.from({ length: wave.width }, (_, x) => {
      const from = timeAt(view, x / wave.width);
      const to = timeAt(view, (x + 1) / wave.width);
      const source = tiles.find((t) => t.from <= from && from < t.to) ?? waves;
      return peakBetween(source, from, to);
    });
    // read stretches cover the hatch in runs, then the peaks go over them in one color
    context.fillStyle = palette.plot;
    let run = -1;
    peaks.forEach((peak, x) => {
      if (peak && run < 0) run = x;
      if ((!peak || x === peaks.length - 1) && run >= 0) {
        const end = peak ? x + 1 : x;
        context.fillRect(wave.x + run, wave.y, end - run, wave.height);
        run = -1;
      }
    });
    context.fillStyle = palette.wave;
    peaks.forEach((peak, x) => {
      if (!peak) return;
      const y0 = Math.floor(middle - peak[1] * half);
      const y1 = Math.ceil(middle - peak[0] * half);
      context.fillRect(wave.x + x, y0, 1, Math.max(1, y1 - y0));
    });
  }

  context.textAlign = "center";
  context.textBaseline = "top";
  const ticks = timeTicks(view, (5 * rem) / axis.width);
  const tenths = ticks.length > 1 && ticks[1] - ticks[0] < 1;
  for (const t of ticks) {
    const x = Math.round(axis.x + acrossOf(view, t) * axis.width);
    context.fillStyle = palette.muted;
    context.fillRect(x, axis.y, dpr, 0.25 * rem);
    const label = formatTime(t, tenths);
    const w = context.measureText(label).width / 2;
    context.fillText(label, Math.min(stage.width - w, Math.max(w, x)), axis.y + 0.3 * rem);
  }

  // the line under the pointer, under the cursor's
  const hovered =
    scene.hover &&
    (inside(spec, scene.hover.x, scene.hover.y) || inside(wave, scene.hover.x, scene.hover.y));
  if (hovered) {
    context.fillStyle = palette.fg;
    context.globalAlpha = 0.5;
    context.fillRect(scene.hover!.x, spec.y, 1, wave.y + wave.height - spec.y);
    context.globalAlpha = 1;
  }

  const across = acrossOf(view, scene.cursor);
  if (across >= 0 && across <= 1) {
    const x = Math.round(spec.x + across * spec.width);
    context.fillStyle = palette.cursor;
    const width = Math.max(1, Math.round(1.5 * dpr));
    context.fillRect(x - Math.floor(width / 2), spec.y, width, wave.y + wave.height - spec.y);
    chip(context, formatTime(scene.cursor, true), x, axis.y + 0.15 * rem, stage, palette, true);
  }

  if (hovered) readout(context, stage, scene, overview, shown, detail);
};

/** A label in a box at `x`, kept on the canvas */
const chip = (
  context: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  stage: Stage,
  palette: Palette,
  dark: boolean
) => {
  const { rem } = stage;
  context.font = `${Math.round(0.7 * rem)}px ui-sans-serif, system-ui, sans-serif`;
  const width = context.measureText(text).width + 0.6 * rem;
  const height = 1.1 * rem;
  const left = Math.min(stage.width - width, Math.max(stage.spectrogram.x, x - width / 2));
  context.fillStyle = dark ? palette.cursor : palette.plot;
  context.globalAlpha = dark ? 1 : 0.9;
  context.fillRect(left, y, width, height);
  context.globalAlpha = 1;
  context.fillStyle = dark ? palette.plot : palette.fg;
  context.textAlign = "left";
  context.textBaseline = "middle";
  context.fillText(text, left + 0.3 * rem, y + height / 2);
};

/** The time, frequency and level under the pointer, or the time and peak over the waveform */
const readout = (
  context: CanvasRenderingContext2D,
  stage: Stage,
  scene: Scene,
  overview: Cells,
  tiles: Cells[],
  detail: boolean
) => {
  const { x, y } = scene.hover!;
  const { spectrogram: spec, waveform: wave, rem } = stage;
  const { layout } = scene.pictures;
  let text: string | null = null;
  if (inside(spec, x, y)) {
    const t = timeAt(scene.view, acrossRect(spec, x));
    const height = (y - spec.y) / spec.height;
    const hz = hzOn(scene.curve, layout.maxHz, height);
    const bin = Math.min(layout.bins - 1, Math.floor((hz / layout.maxHz) * layout.bins));
    const source = (detail && tiles.find((c) => c.from <= t && t < c.to)) || overview;
    const byte = levelAt(source, t, bin);
    const db =
      byte === null
        ? "not read"
        : `${Math.round((source.minDb ?? -160) + (byte / 255) * ((source.maxDb ?? 0) - (source.minDb ?? -160)))} dB`;
    text = `${formatTime(t, true)} · ${formatHz(hz)} · ${db}`;
  } else if (inside(wave, x, y) && scene.pictures.waveform) {
    const t = timeAt(scene.view, acrossRect(wave, x));
    const span = scene.view.span / wave.width;
    const peak = peakBetween(scene.pictures.waveform, t, t + span);
    const level = peak ? Math.max(-peak[0], peak[1]) : 0;
    const db = !peak
      ? "not read"
      : level > 0
        ? `peak ${(20 * Math.log10(level)).toFixed(1)} dBFS`
        : "silent";
    text = `${formatTime(t, true)} · ${db}`;
  }
  if (!text) return;
  const below = y + 1.6 * rem < stage.height ? y + 0.6 * rem : y - 1.7 * rem;
  chip(
    context,
    text,
    x + 0.8 * rem + context.measureText(text).width / 2,
    below,
    stage,
    scene.palette,
    false
  );
};
