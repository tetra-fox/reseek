import { curveMonotoneX } from "d3-shape";

import { formatBytes, formatSpeed, formatWait } from "#lib/format.js";

/** What a series measures, which decides how its values read */
export type Unit = "bytes" | "count" | "rate" | "ratio" | "seconds" | "speed";

const DAY = 24 * 60 * 60_000;

const TIME = new Intl.DateTimeFormat([], { hour: "2-digit", minute: "2-digit" });
const DATE = new Intl.DateTimeFormat([], { day: "numeric", month: "short" });
const DATE_YEAR = new Intl.DateTimeFormat([], { day: "numeric", month: "short", year: "2-digit" });

/** How a time on the axis reads: the time of day within a day, else the date */
export const timeLabel = (min: number, max: number) => {
  if (max - min <= DAY) return (t: number) => TIME.format(t);
  const years = new Date(min).getFullYear() !== new Date(max).getFullYear();
  return (t: number) => (years ? DATE_YEAR : DATE).format(t);
};

/**
 * How every line and area chart joins its points: a smooth curve through each one that never
 * overshoots them, so a rate never dips below 0 or rises past its peak between two points
 */
export const chartCurve = curveMonotoneX;

/** The size of a chart's axis labels, in rem */
const LABEL_REM = 0.6875;

/** How a chart's axis labels are drawn: small, muted, with figures of one width */
export const tickLabel = {
  class: "fill-fg-muted",
  style: `font-size: ${LABEL_REM}rem; font-variant-numeric: tabular-nums`
};

/**
 * Room around a chart's plot for its axes, in pixels at `remPx` to the rem: on the left the
 * widest of `labels`, the value labels the axis can show, below one line of time labels, and above
 * half a line for the top label, which is centered on its tick. A figure is taken as 0.62 of the
 * text size wide, a little more than a tabular digit
 */
export const axisPadding = (labels: string[], remPx: number) => {
  const font = LABEL_REM * remPx;
  const widest = Math.max(...labels.map((l) => l.length));
  return {
    top: Math.ceil(font),
    right: Math.ceil(remPx / 2),
    bottom: Math.ceil(font * 1.4 + remPx / 2),
    left: Math.ceil(widest * font * 0.62 + remPx / 2)
  };
};

/** How a value of `unit` reads */
export const valueLabel = (unit: Unit) => {
  switch (unit) {
    case "bytes":
      return (n: number) => formatBytes(n, 1);
    case "count":
      return (n: number) => n.toLocaleString();
    case "rate":
      return (n: number) => `${n.toFixed(1)}%`;
    case "ratio":
      return (n: number) => n.toFixed(2);
    case "seconds":
      return (n: number) => formatWait(n);
    case "speed":
      return (n: number) => formatSpeed(n);
  }
};

/** How many steps a value axis divides into at most */
const MOST_STEPS = 5;

const DECADE = [1, 2, 5, 10];
/** Steps within a power of 1024, each landing on a label that reads whole: 512 MB, 1 GB, 1.5 GB */
const BINARY = [1, 2, 5, 10, 20, 50, 100, 200, 512, 1024];
/** Steps up to a minute, as a clock has them */
const CLOCK = [1, 2, 5, 10, 15, 30, 60];

/** `scale` times the first of `steps` that reaches `least` */
const firstStep = (steps: number[], scale: number, least: number) =>
  scale * steps.find((step) => step * scale >= least)!;

const decimalStep = (least: number) =>
  firstStep(DECADE, 10 ** Math.floor(Math.log10(least)), least);

/** The smallest round step of `unit` at or above `least`, never finer than its label can show */
const stepOf = (unit: Unit, least: number) => {
  switch (unit) {
    case "bytes":
    case "speed":
      return firstStep(BINARY, 1024 ** Math.max(0, Math.floor(Math.log2(least) / 10)), least);
    case "count":
      return Math.max(1, decimalStep(least));
    case "rate":
      return Math.max(0.1, decimalStep(least));
    case "ratio":
      return Math.max(0.01, decimalStep(least));
    case "seconds":
      return least <= 60 ? firstStep(CLOCK, 1, least) : 60 * decimalStep(least / 60);
  }
};

/**
 * The value ticks of a chart in `unit` whose values reach `top`, from 0 to the first round step at
 * or past it, which is where the axis ends; a chart of nothing but zeroes has the one tick
 */
export const axisTicks = (unit: Unit, top: number) => {
  if (top <= 0) return [0];
  const step = stepOf(unit, top / MOST_STEPS);
  return Array.from({ length: Math.ceil(top / step) + 1 }, (_, i) => i * step);
};
