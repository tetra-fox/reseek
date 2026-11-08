import { viewport } from "#lib/ui/viewport.svelte.js";

/** The width of a tree line's open-and-close toggle, in rem */
export const TOGGLE_REM = 1.25;

/** The gap between the parts of a tree line: its tick, its toggle, its icon and its name, in rem */
export const GAP_REM = 0.375;

/** A tick, a 1rem checkbox, and the gap after it, in rem */
export const TICK_REM = 1 + GAP_REM;

/** How far a tree line at depth 0 starts in from its row's edge, in rem */
const MARGIN_REM = 0.5;

/** How far each level sets a line in, in rem: shorter on a phone so a deep name keeps its room */
export const stepRem = () => (viewport.phone ? 0.75 : 1.25);

/** Where a tree line at `depth` starts in from its row's edge, in rem */
export const lineStart = (depth: number) => MARGIN_REM + depth * stepRem();

/**
 * Where an item at `depth` starts in, in rem: past the toggle and the gap that start a folder line
 * at its depth, so its cells line up under the folder line's content
 */
export const itemStart = (depth: number) => lineStart(depth) + TOGGLE_REM + GAP_REM;

/**
 * Where a line at `depth` starts in as CSS, each level a `step`, a CSS length: a folder's line at
 * the margin, an item's past the toggle as `itemStart` has it
 */
export const insetCss = (depth: number, kind: "folder" | "item", step: string) =>
  `calc(${kind === "item" ? itemStart(0) : MARGIN_REM}rem + ${depth} * ${step})`;
