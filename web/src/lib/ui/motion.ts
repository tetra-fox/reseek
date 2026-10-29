import { cubicOut } from "svelte/easing";
import { prefersReducedMotion } from "svelte/motion";
import { slide } from "svelte/transition";

/**
 * A motion token in ms, for motion run from script. 0 when reduced motion is asked for. The
 * built stylesheet may write it in seconds (".25s")
 */
const token = (name: string) => {
  if (prefersReducedMotion.current) return 0;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return parseFloat(value) * (value.endsWith("ms") ? 1 : 1000);
};

/** The --motion-fast token in ms, for a small thing appearing or leaving */
export const motionFast = () => token("--motion-fast");

/** The --motion-slow token in ms, for something larger moving into place */
export const motionSlow = () => token("--motion-slow");

/**
 * The --ease-out-soft token, cubic-bezier(0.2, 0, 0, 1), as an easing function for motion run from
 * script: the curve's x solved for `t` by Newton's method, then its y
 */
export const easeOutSoft = (t: number) => {
  const [x1, y1, x2, y2] = [0.2, 0, 0, 1];
  const at = (a: number, b: number, s: number) =>
    3 * a * s * (1 - s) ** 2 + 3 * b * s * s * (1 - s) + s ** 3;
  let s = t;
  for (let i = 0; i < 8; i++) {
    const slope = 3 * x1 * (1 - s) ** 2 + 6 * (x2 - x1) * s * (1 - s) + 3 * (1 - x2) * s * s;
    if (Math.abs(slope) < 1e-6) break;
    s -= (at(x1, x2, s) - t) / slope;
  }
  return at(y1, y2, Math.min(1, Math.max(0, s)));
};

/**
 * How a disclosure's content opens and closes: its height grows from nothing or shrinks to it,
 * over --motion-slow with the soft ease-out; at once when reduced motion is asked for. For
 * `transition:disclose` on what a section, a folder or an accordion shows while open
 */
export const disclose = (node: Element) =>
  slide(node, { duration: motionSlow(), easing: easeOutSoft });

/**
 * How a chart's marks grow in as it first shows and move to new figures: a short tween, or none
 * when reduced motion is asked for. Read once as a mark mounts
 */
export const chartMotion = () =>
  prefersReducedMotion.current
    ? ("none" as const)
    : { type: "tween" as const, duration: 500, easing: cubicOut };

/**
 * Makes a change to the page as one cross-fade where the browser can, the old page fading into
 * the new; at once where it can't or reduced motion is asked for. Resolves once the change is made
 */
export const crossFade = async (update: () => void | Promise<void>): Promise<void> => {
  if (prefersReducedMotion.current || typeof document.startViewTransition !== "function") {
    await update();
    return;
  }
  await document.startViewTransition(update).updateCallbackDone;
};
