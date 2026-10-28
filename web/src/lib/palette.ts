import { computeCommandScore } from "bits-ui";

/** An entry's value in the palette: its label, then its id to keep two alike labels apart */
export const paletteValue = (label: string, id: string) => `${label}|${id}`;

/** The ids of what the typed text can be named as, listed after everything it matches */
const TYPED = "|do:";

/**
 * How well an entry matches what was typed, 0 for not at all. The label matches loosely, as the
 * command list's own scoring does; the keywords (a setting's option names, a hint) only hold the
 * text whole, so a setting is not listed for a few stray letters of its keys. The typed actions
 * always match, scored below everything else
 */
export const paletteScore = (value: string, search: string, keywords: string[] = []): number => {
  if (value.includes(TYPED)) return 0.001;
  const label = computeCommandScore(value.slice(0, value.lastIndexOf("|")), search);
  if (label > 0) return label;
  const typed = search.trim().toLowerCase();
  return typed && keywords.some((k) => k.toLowerCase().includes(typed)) ? 0.01 : 0;
};
