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

const COMPACT = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });

/** A number short: 1.2K, 3.4M, 5.6B */
const compact = (n: number) => COMPACT.format(n);

/** How a value of `unit` reads; a mix of units reads as a short number */
export const valueLabel = (unit: Unit | null) => {
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
    default:
      return compact;
  }
};
