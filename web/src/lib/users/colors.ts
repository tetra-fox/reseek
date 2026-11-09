/**
 * The hues a username's color is picked from: twelve steps around the color wheel, so names
 * next to each other in a chat are told apart at a glance. The lightness and chroma below keep
 * each one readable as text on both themes' surfaces
 */
export const NAME_HUES = [15, 45, 75, 110, 145, 175, 200, 230, 260, 290, 320, 350] as const;

/** Lightness and chroma of a name's color on the light theme's white surfaces */
export const LIGHT = { l: 0.48, c: 0.15 } as const;
/** Lightness and chroma of a name's color on the dark theme's surfaces */
export const DARK = { l: 0.8, c: 0.12 } as const;

/**
 * The 32-bit FNV-1a hash of a name's UTF-16 code units: fast, stable across sessions and
 * browsers, and spreads names that differ by one letter
 */
const fnv1a = (text: string) => {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
};

/** The hue a username always gets */
export const nameHue = (name: string): number => NAME_HUES[fnv1a(name) % NAME_HUES.length];

/**
 * A username's text color, the same everywhere it shows: one of the hues at the lightness that
 * reads on the theme in use (`light-dark` follows the page's color scheme)
 */
export const nameColor = (name: string): string => {
  const h = nameHue(name);
  return `light-dark(oklch(${LIGHT.l} ${LIGHT.c} ${h}), oklch(${DARK.l} ${DARK.c} ${h}))`;
};

/**
 * The hue of each color a group can be shown in, the daemon's names in its order, taken from the
 * hues names are drawn in so a group's color reads as text like a name's; gray has none
 */
const GROUP_HUES = new Map<string, number | null>([
  ["blue", 230],
  ["red", 15],
  ["orange", 45],
  ["amber", 75],
  ["lime", 110],
  ["green", 145],
  ["teal", 175],
  ["cyan", 200],
  ["indigo", 260],
  ["violet", 290],
  ["pink", 350],
  ["gray", null]
]);

/** The colors a group can be given, in the order a picker offers them */
export const GROUP_COLORS = [...GROUP_HUES.keys()];

/** The CSS color of a group's color name, null for a group without one or a name not known here */
export const groupColor = (color: string | null | undefined): string | null => {
  const hue = color ? GROUP_HUES.get(color) : undefined;
  if (hue === undefined) return null;
  const [light, dark] = hue === null ? [0, 0] : [LIGHT.c, DARK.c];
  return `light-dark(oklch(${LIGHT.l} ${light} ${hue ?? 0}), oklch(${DARK.l} ${dark} ${hue ?? 0}))`;
};
