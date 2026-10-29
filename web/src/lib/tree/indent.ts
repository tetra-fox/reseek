import { viewport } from "#lib/ui/viewport.svelte.js";

/** The width of a tree line's open-and-close toggle, in rem */
export const TOGGLE_REM = 1.25;

/** The gap between the parts of a tree line: its tick, its toggle, its icon and its name, in rem */
export const GAP_REM = 0.375;

/** A folder line's height in rem, which pinned folder lines stack by */
export const FOLDER_LINE_REM = 2.25;

/** A tick, a 1rem checkbox, and the gap after it, in rem */
export const TICK_REM = 1 + GAP_REM;

/**
 * Where a tree line at `depth` starts in from its row's edge, in rem: a step a level past a
 * margin, the step shorter on a phone so a deep name keeps its room
 */
export const lineStart = (depth: number) => 0.5 + depth * (viewport.phone ? 0.75 : 1.25);
