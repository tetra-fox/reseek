import { type Scale, SCALES } from "./analysis";

/** The lowest frequency a log axis shows, in Hz; linear and mel axes start at 0 */
export const LOG_MIN_HZ = 20;

/** The frequency at the bottom of an axis on `scale` */
export const minHzOf = (scale: Scale) => (scale === "log" ? LOG_MIN_HZ : 0);

/** The mel pitch of `hz`, by O'Shaughnessy's formula: 2595 log10(1 + f / 700) */
const mel = (hz: number) => 2595 * Math.log10(1 + hz / 700);

/** The frequency of a mel pitch, the inverse of `mel` */
const hzOfMel = (pitch: number) => 700 * (10 ** (pitch / 2595) - 1);

/**
 * Where `hz` falls on an axis on `scale` up to `maxHz`, from 0 at its top to 1 at its bottom:
 * hertz the same height on a linear axis, octaves on a log one, pitch as heard on a mel one. A
 * frequency off the axis sits at its edge
 */
export const heightOf = (scale: Scale, maxHz: number, hz: number): number => {
  const min = minHzOf(scale);
  const position =
    scale === "log"
      ? Math.log(Math.max(hz, min) / min) / Math.log(maxHz / min)
      : scale === "mel"
        ? (mel(Math.max(hz, min)) - mel(min)) / (mel(maxHz) - mel(min))
        : (hz - min) / (maxHz - min);
  return 1 - Math.min(1, Math.max(0, position));
};

/** The frequency at `height` on an axis on `scale` up to `maxHz`, the inverse of `heightOf` */
export const hzAt = (scale: Scale, maxHz: number, height: number): number => {
  const position = 1 - Math.min(1, Math.max(0, height));
  const min = minHzOf(scale);
  if (scale === "log") return min * (maxHz / min) ** position;
  if (scale === "mel") return hzOfMel(mel(min) + position * (mel(maxHz) - mel(min)));
  return min + position * (maxHz - min);
};

/** The middle of the daemon's frequency bin `bin` of `bins` spanning 0 to `maxHz` */
export const binHz = (bin: number, bins: number, maxHz: number) => ((bin + 0.5) * maxHz) / bins;

/** The frequencies an axis on `scale` is labeled at, up to `maxHz` */
export const frequencyTicks = (scale: Scale, maxHz: number): number[] => {
  const ticks = {
    linear: [0, 5_000, 10_000, 15_000, 20_000, 30_000, 40_000, 60_000, 80_000],
    log: [50, 100, 200, 500, 1_000, 2_000, 5_000, 10_000, 20_000, 50_000],
    mel: [0, 500, 1_000, 2_000, 4_000, 8_000, 16_000, 32_000, 64_000]
  }[scale];
  return ticks.filter((hz) => hz >= minHzOf(scale) && hz <= maxHz);
};

/** A frequency in words for a label: "16 kHz", "19.5 kHz", "500 Hz" */
export const formatHz = (hz: number): string =>
  hz >= 1000 ? `${Number((hz / 1000).toFixed(hz >= 10_000 ? 1 : 2))} kHz` : `${Math.round(hz)} Hz`;

/**
 * A frequency axis as a mix of the three scales, each weight a scale's share in where a frequency
 * sits. One scale at 1 is that scale's own axis; weights between are an axis on its way from one
 * scale to another
 */
export type Curve = Record<Scale, number>;

/** The axis of `scale` alone */
export const pure = (scale: Scale): Curve => ({ linear: 0, log: 0, mel: 0, [scale]: 1 });

/** The scale `curve` is entirely, null while it is between scales */
export const scaleOf = (curve: Curve): Scale | null =>
  SCALES.find(({ id }) => curve[id] === 1)?.id ?? null;

/** The axis `t` of the way from `from` to `to`, exactly `from` at 0 and `to` at 1 */
export const mix = (from: Curve, to: Curve, t: number): Curve => ({
  linear: (1 - t) * from.linear + t * to.linear,
  log: (1 - t) * from.log + t * to.log,
  mel: (1 - t) * from.mel + t * to.mel
});

/**
 * Where `hz` falls on an axis on `curve` up to `maxHz`, from 0 at its top to 1 at its bottom: the
 * heights the scales give it, weighed. Each scale's height falls as the frequency rises, so a
 * mix of them does too
 */
export const heightOn = (curve: Curve, maxHz: number, hz: number): number => {
  let height = 0;
  for (const { id } of SCALES) if (curve[id] > 0) height += curve[id] * heightOf(id, maxHz, hz);
  return height;
};

/** The frequency at `height` on an axis on `curve`, the inverse of `heightOn` */
export const hzOn = (curve: Curve, maxHz: number, height: number): number => {
  const scale = scaleOf(curve);
  if (scale) return hzAt(scale, maxHz, height);
  // a mix has no closed inverse; halving the range 48 times is far below a hertz
  let low = 0;
  let high = maxHz;
  for (let i = 0; i < 48; i++) {
    const middle = (low + high) / 2;
    if (heightOn(curve, maxHz, middle) > height) low = middle;
    else high = middle;
  }
  return (low + high) / 2;
};

/**
 * Which of `bins` linear bins each of `rows` rows of an axis on `curve` reads, as a position
 * between bin middles: bin 2.25 is a quarter of the way from bin 2 to bin 3. A row is placed
 * between the two bins whose heights are around it, found in one sweep down the rows and the
 * bins together. Rows over the top bin's middle read the top bin, rows under the bottom one's
 * the bottom bin
 */
export const rowBins = (curve: Curve, maxHz: number, bins: number, rows: number): Float32Array => {
  const heights = new Float64Array(bins);
  for (let bin = 0; bin < bins; bin++)
    heights[bin] = heightOn(curve, maxHz, binHz(bin, bins, maxHz));
  const table = new Float32Array(rows);
  let bin = bins - 1;
  for (let row = 0; row < rows; row++) {
    const height = (row + 0.5) / rows;
    while (bin > 0 && heights[bin - 1] <= height) bin--;
    // the bin below the row now sits under it, or is the bottom bin
    if (heights[bin] >= height || bin === 0) table[row] = bin;
    else table[row] = bin - (height - heights[bin]) / (heights[bin - 1] - heights[bin]);
  }
  return table;
};

/** A labeled frequency on an axis: where it sits, from 0 at the top to 1 at the bottom, and how opaque it is drawn */
export type Tick = { hz: number; at: number; alpha: number };

/** The frequencies an axis on `scale` `length` long labels, each at least `room` from the last */
const spacedTicks = (scale: Scale, maxHz: number, length: number, room: number): number[] => {
  let last = Number.POSITIVE_INFINITY;
  return frequencyTicks(scale, maxHz).filter((hz) => {
    const at = heightOf(scale, maxHz, hz) * length;
    if (last - at < room) return false;
    last = at;
    return true;
  });
};

/**
 * The labeled frequencies of an axis `length` long on `curve`. Each scale labels its own
 * frequencies at least `room` apart, and between scales a label shows as far as the scales that
 * have it weigh more than half: the old scale's own labels are gone by the middle of a move and
 * the new one's come in after it, while those both have stay. Labels that come nearer than `room`
 * on the way fade out, gone by 0.85 of it, the label with more weight or else the lower one kept;
 * a label holds others back only as far as it is clear itself
 */
export const axisTicks = (curve: Curve, maxHz: number, length: number, room: number): Tick[] => {
  const weights = new Map<number, number>();
  for (const { id } of SCALES) {
    if (curve[id] <= 0) continue;
    for (const hz of spacedTicks(id, maxHz, length, room)) {
      weights.set(hz, (weights.get(hz) ?? 0) + curve[id]);
    }
  }
  // the distance labels fade over, none on a scale and widening as the axis leaves it
  const fade = room * Math.min(0.15, 1 - Math.max(curve.linear, curve.log, curve.mel));
  const apart = (gap: number) =>
    fade > 0 ? Math.min(1, Math.max(0, (gap - room + fade) / fade)) : gap >= room ? 1 : 0;
  const kept: (Tick & { clear: number })[] = [];
  for (const [hz, weight] of [...weights].sort(([a, wa], [b, wb]) => wb - wa || a - b)) {
    const at = heightOn(curve, maxHz, hz);
    let clear = 1;
    for (const k of kept) clear *= 1 - k.clear * (1 - apart(Math.abs(k.at - at) * length));
    const alpha = clear * Math.min(1, Math.max(0, 2 * weight - 1));
    if (alpha > 0) kept.push({ hz, at, alpha, clear });
  }
  return kept.sort((a, b) => a.hz - b.hz).map(({ hz, at, alpha }) => ({ hz, at, alpha }));
};
