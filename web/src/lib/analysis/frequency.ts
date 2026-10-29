import type { Scale } from "./analysis";

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

/**
 * Which bins a row at `height` on the axis reads, for drawing levels kept over linear bins on any
 * axis: the bin below the row's frequency, the one above, and how far between them it is
 */
export const binsAt = (
  scale: Scale,
  maxHz: number,
  bins: number,
  height: number
): { below: number; above: number; share: number } => {
  const at = (hzAt(scale, maxHz, height) / maxHz) * bins - 0.5;
  if (at >= bins - 1) return { below: bins - 1, above: bins - 1, share: 0 };
  const below = Math.max(0, Math.floor(at));
  return { below, above: below + 1, share: Math.max(0, at - below) };
};

/** The frequencies an axis on `scale` is labelled at, up to `maxHz` */
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
