import type { Cells } from "./pictures.svelte";
import type { View } from "./timeview";

/** The colors a spectrogram is drawn in, read from the theme */
export type Palette = {
  /** the level map from quiet to loud, as red, green and blue */
  levels: [number, number, number][];
  plot: string;
  unread: string;
  fg: string;
  muted: string;
  wave: string;
  cursor: string;
};

/** A CSS color as red, green and blue, whatever notation the theme writes it in */
const rgb = (color: string): [number, number, number] => {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const context = canvas.getContext("2d");
  if (!context) return [0, 0, 0];
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  const [r, g, b] = context.getImageData(0, 0, 1, 1).data;
  return [r, g, b];
};

/** The palette the theme gives `element` */
export const readPalette = (element: Element): Palette => {
  const style = getComputedStyle(element);
  const token = (name: string) => style.getPropertyValue(`--color-${name}`).trim();
  return {
    levels: Array.from({ length: 9 }, (_, i) => rgb(token(`level-${i}`))),
    plot: token("plot"),
    unread: token("plot-unread"),
    fg: token("plot-fg"),
    muted: token("plot-muted"),
    wave: token("plot-wave"),
    cursor: token("plot-cursor")
  };
};

/** How many dB below the loudest level the colors cover */
export const RANGE_DB = 120;

/**
 * The color of every level byte, as RGBA: `top` (the loudest byte in the file) is the loudest
 * color and `RANGE_DB` below it the quietest. A byte covers `dbPerByte` dB
 */
export const levelColors = (levels: Palette["levels"], top: number, dbPerByte: number) => {
  const lut = new Uint8ClampedArray(256 * 4);
  const range = RANGE_DB / dbPerByte;
  for (let byte = 0; byte < 256; byte++) {
    const t = Math.min(1, Math.max(0, 1 - (top - byte) / range)) * (levels.length - 1);
    const index = Math.min(levels.length - 2, Math.floor(t));
    const share = t - index;
    for (let c = 0; c < 3; c++) {
      lut[byte * 4 + c] = levels[index][c] + (levels[index + 1][c] - levels[index][c]) * share;
    }
    lut[byte * 4 + 3] = 255;
  }
  return lut;
};

/** Which cells of each set of cells were read, worked out once a set */
const masks = new WeakMap<Cells, Uint8Array>();

/** Whether cell `index` of `cells` was read */
const readAt = (cells: Cells, index: number) => {
  let mask = masks.get(cells);
  if (!mask) {
    mask = new Uint8Array(cells.count).fill(1);
    for (const [first, end] of cells.unread) mask.fill(0, first, end);
    masks.set(cells, mask);
  }
  return mask[index] === 1;
};

/**
 * Spectrogram columns as an image one pixel a column and one row a linear bin, bin 0 on row 0,
 * which an axis on any scale is drawn from by `warp`. A column the analysis did not read is left
 * clear
 */
export const binRaster = (cells: Cells, colors: Uint8ClampedArray): HTMLCanvasElement => {
  const canvas = document.createElement("canvas");
  const bins = cells.width;
  canvas.width = Math.max(1, cells.count);
  canvas.height = Math.max(1, bins);
  const context = canvas.getContext("2d");
  if (!context || !cells.count) return canvas;
  const image = context.createImageData(cells.count, bins);
  for (let x = 0; x < cells.count; x++) {
    if (!readAt(cells, x)) continue;
    for (let bin = 0; bin < bins; bin++) {
      const level = cells.bytes[x * bins + bin];
      const at = (bin * cells.count + x) * 4;
      image.data[at] = colors[level * 4];
      image.data[at + 1] = colors[level * 4 + 1];
      image.data[at + 2] = colors[level * 4 + 2];
      image.data[at + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
  return canvas;
};

/** A rectangle on a canvas, in device pixels */
export type Rect = { x: number; y: number; width: number; height: number };

/**
 * Draws columns `from` to `to` of a bin raster into `rect`, a row of pixels at a time: row `y`
 * reads the bin position `rows[y]`, which the canvas's smoothing blends from the two bins around
 * it. `rows` has a row for each of the rect's
 */
export const warp = (
  context: CanvasRenderingContext2D,
  raster: HTMLCanvasElement,
  rows: Float32Array,
  from: number,
  to: number,
  rect: Rect
) => {
  for (let y = 0; y < rows.length; y++) {
    context.drawImage(raster, from, rows[y], to - from, 1, rect.x, rect.y + y, rect.width, 1);
  }
};

/**
 * Draws the part of `raster` (made from `cells`) that falls in `view` into `rect`: the image as
 * it is, or with `rows` a bin raster warped onto the axis they were made for
 */
export const drawRaster = (
  context: CanvasRenderingContext2D,
  raster: HTMLCanvasElement,
  cells: Cells,
  view: View,
  rect: Rect,
  rows?: Float32Array
) => {
  const from = Math.max(view.start, cells.from);
  const to = Math.min(view.start + view.span, cells.to);
  if (to <= from || !cells.count) return;
  const seconds = (cells.to - cells.from) / cells.count;
  const target = {
    x: rect.x + ((from - view.start) / view.span) * rect.width,
    y: rect.y,
    width: ((to - from) / view.span) * rect.width,
    height: rect.height
  };
  const first = (from - cells.from) / seconds;
  const end = (to - cells.from) / seconds;
  if (rows) {
    warp(context, raster, rows, first, end, target);
    return;
  }
  context.drawImage(
    raster,
    first,
    0,
    end - first,
    raster.height,
    target.x,
    target.y,
    target.width,
    target.height
  );
};

/** The loudest level byte of each set of columns, found once a set */
const tops = new WeakMap<Cells, number>();

/** The loudest level byte of a spectrogram's columns */
export const loudest = (cells: Cells) => {
  let top = tops.get(cells);
  if (top === undefined) {
    top = 0;
    for (const byte of cells.bytes) if (byte > top) top = byte;
    tops.set(cells, top);
  }
  return top;
};

/**
 * The lowest and highest sample from `from` to `to` seconds in waveform `cells`, from -1 to 1;
 * null where no read bucket falls
 */
export const peakBetween = (cells: Cells, from: number, to: number): [number, number] | null => {
  const seconds = (cells.to - cells.from) / cells.count;
  const first = Math.max(0, Math.floor((from - cells.from) / seconds));
  const end = Math.min(cells.count, Math.max(first + 1, Math.ceil((to - cells.from) / seconds)));
  let low = 1;
  let high = -1;
  for (let i = first; i < end; i++) {
    if (!readAt(cells, i)) continue;
    // the bytes are signed
    const min = ((cells.bytes[i * 2] << 24) >> 24) / 127;
    const max = ((cells.bytes[i * 2 + 1] << 24) >> 24) / 127;
    if (min < low) low = min;
    if (max > high) high = max;
  }
  return high < low ? null : [low, high];
};

/** The level byte at `t` seconds in bin `bin` of spectrogram `cells`, null where none was read */
export const levelAt = (cells: Cells, t: number, bin: number): number | null => {
  if (t < cells.from || t >= cells.to || !cells.count) return null;
  const index = Math.floor(((t - cells.from) / (cells.to - cells.from)) * cells.count);
  return readAt(cells, index) ? cells.bytes[index * cells.width + bin] : null;
};
