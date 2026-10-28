// Shared seed palette for the dither chart family. Mirrors the seeds in
// `dither-chart.tsx` so a series rendered through the composable engine reads
// with the exact same fill / line / star hues as the legacy sparkline.

export type Rgb = [number, number, number]

export type Seed = { fill: Rgb; line: Rgb; star: Rgb }

// changed for reseek: upstream's named seed palette (PALETTE, seedOfColor, isDitherColor) is left
// out; a series takes its colour from the app's theme tokens instead

export const rgb = ([r, g, b]: Rgb, k = 1, a = 1) =>
  `rgba(${Math.round(r * k)},${Math.round(g * k)},${Math.round(b * k)},${a})`
