import { prefersReducedMotion } from "svelte/motion";

/** The --motion-fast token in ms, for motion run from script. 0 when reduced motion is asked for */
export const motionFast = () =>
  prefersReducedMotion.current
    ? 0
    : parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--motion-fast"));
