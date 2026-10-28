import { formatBytes, formatSpeed, formatWait } from "#lib/format.js";

/** What a series measures, which decides how its values read */
export type Unit = "bytes" | "count" | "rate" | "ratio" | "seconds" | "speed";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** the tick spacings a time axis picks from */
const STEPS = [
  MINUTE,
  5 * MINUTE,
  15 * MINUTE,
  30 * MINUTE,
  HOUR,
  3 * HOUR,
  6 * HOUR,
  12 * HOUR,
  DAY,
  2 * DAY,
  3 * DAY,
  7 * DAY,
  14 * DAY,
  30 * DAY,
  90 * DAY,
  180 * DAY,
  365 * DAY
];

/** The ticks a step puts between `min` and `max`, on its round times */
const ticksOf = (min: number, max: number, step: number): number[] => {
  // whole days and longer start at local midnight, shorter steps at round UTC times
  const offset = step >= DAY ? new Date(min).getTimezoneOffset() * MINUTE : 0;
  const first = Math.ceil((min - offset) / step) * step + offset;
  const ticks: number[] = [];
  for (let t = first; t <= max; t += step) ticks.push(t);
  return ticks;
};

/**
 * Ticks for a time axis from `min` to `max`: about `count` of them on round times (whole hours,
 * whole days) rather than on wherever the span happens to start, and at least two, so a narrow
 * chart still says where its curve lies in time. None for a span of nothing
 */
export const timeTicks = (min: number, max: number, count = 11): number[] => {
  if (!(max > min)) return [];
  let at = STEPS.findIndex((s) => (max - min) / s <= count - 1);
  if (at === -1) at = STEPS.length - 1;
  let ticks = ticksOf(min, max, STEPS[at]);
  while (ticks.length < 2 && at > 0) ticks = ticksOf(min, max, STEPS[--at]);
  return ticks;
};

/** How a time on the axis reads: the time of day within a day, else the date */
export const timeLabel = (min: number, max: number) => {
  if (max - min <= DAY) {
    return (t: number) =>
      new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  const years = new Date(min).getFullYear() !== new Date(max).getFullYear();
  return (t: number) =>
    new Date(t).toLocaleDateString([], {
      day: "numeric",
      month: "short",
      ...(years ? { year: "2-digit" } : {})
    });
};

/** A round top for an axis whose values reach `max`, and the ticks up to it */
export const valueTicks = (max: number, count = 4): number[] => {
  if (!(max > 0)) return [0];
  const raw = max / count;
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * magnitude).find((s) => s >= raw)!;
  return Array.from({ length: Math.ceil(max / step) + 1 }, (_, i) => i * step);
};

/** A number short: 1.2K, 3.4M, 5.6B */
const compact = (n: number) =>
  new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(n);

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
