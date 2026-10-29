/** The stretch of a file's time a view shows: from `start`, `span` seconds long */
export type View = { start: number; span: number };

/** The shortest stretch a view zooms in to, in seconds */
export const MIN_SPAN = 2;

/** A view of the whole of a file `duration` seconds long */
export const whole = (duration: number): View => ({ start: 0, span: Math.max(duration, 0) });

/** `view` kept inside a file `duration` seconds long and no shorter than it can show */
export const clamp = (view: View, duration: number): View => {
  const span = Math.min(duration, Math.max(Math.min(MIN_SPAN, duration), view.span));
  const start = Math.min(duration - span, Math.max(0, view.start));
  return { start, span };
};

/** The time at `across` the view, from 0 at its left edge to 1 at its right */
export const timeAt = (view: View, across: number) => view.start + across * view.span;

/** How far across the view a time falls, from 0 at its left edge to 1 at its right */
export const acrossOf = (view: View, t: number) =>
  view.span > 0 ? (t - view.start) / view.span : 0;

/**
 * `view` zoomed by `factor` (above 1 in, below 1 out) around the point `across` it, which keeps
 * the time under that point where it is
 */
export const zoomAround = (view: View, factor: number, across: number, duration: number): View => {
  const anchor = timeAt(view, across);
  const span = clamp({ start: 0, span: view.span / factor }, duration).span;
  return clamp({ start: anchor - across * span, span }, duration);
};

/** `view` moved later by `seconds`, earlier when negative */
export const panBy = (view: View, seconds: number, duration: number): View =>
  clamp({ start: view.start + seconds, span: view.span }, duration);

/** `view` with `t` centered in it */
export const centerOn = (view: View, t: number, duration: number): View =>
  clamp({ start: t - view.span / 2, span: view.span }, duration);

/**
 * `view` moved so a moving time `t` stays in sight: unchanged while it is inside, and once it
 * passes the right tenth or falls before the start, a page on with `t` a tenth in
 */
export const follow = (view: View, t: number, duration: number): View => {
  const across = acrossOf(view, t);
  if (across >= 0 && across <= 0.9) return view;
  return clamp({ start: t - view.span * 0.1, span: view.span }, duration);
};

/** Whether `view` shows all of a file `duration` seconds long */
export const showsAll = (view: View, duration: number) => view.span >= duration - 1e-9;

/**
 * The analysed window at time `t`: the one whose second holds it, or where the file was read in
 * samples and `t` falls between them, the one whose middle is nearest
 */
export const windowAt = (starts: number[], t: number): number => {
  if (!starts.length) return -1;
  // starts are in time order
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if (starts[middle] <= t) low = middle;
    else high = middle - 1;
  }
  if (starts[low] <= t && t < starts[low] + 1) return low;
  const after = starts[low] <= t ? low + 1 : low;
  const before = after - 1;
  if (after >= starts.length) return before;
  if (before < 0) return after;
  return t - (starts[before] + 0.5) <= starts[after] + 0.5 - t ? before : after;
};

/** The steps a time axis is labelled at, in seconds, the shortest first */
const STEPS = [0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600];

/** The times a time axis over `view` is labelled at, at least `apart` of its width apart */
export const timeTicks = (view: View, apart: number): number[] => {
  const step = STEPS.find((s) => s / view.span >= apart) ?? STEPS[STEPS.length - 1];
  const first = Math.ceil(view.start / step - 1e-9) * step;
  const ticks: number[] = [];
  for (let t = first; t <= view.start + view.span + 1e-9; t += step) {
    // adding 0 turns a -0 from rounding into 0
    ticks.push(Math.round(t * 10) / 10 + 0);
  }
  return ticks;
};

/** A time for an axis or a readout: "1:05", "1:05.3" when tenths matter, "1:02:05" past an hour */
export const formatTime = (seconds: number, tenths = false): string => {
  const whole = tenths ? Math.floor(seconds * 10) / 10 : Math.floor(seconds);
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = whole % 60;
  const sec = tenths ? s.toFixed(1).padStart(4, "0") : String(Math.floor(s)).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
};
