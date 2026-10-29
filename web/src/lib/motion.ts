import { cubicOut } from "svelte/easing";
import { prefersReducedMotion } from "svelte/motion";

/** The --motion-fast token in ms, for motion run from script. 0 when reduced motion is asked for */
export const motionFast = () =>
  prefersReducedMotion.current
    ? 0
    : parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--motion-fast"));

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
