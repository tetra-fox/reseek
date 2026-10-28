import { createSubscriber } from "svelte/reactivity";

import type { Rgb } from "#lib/vendor/dither-kit/palette.js";

// the theme switches by a class or a style on the root, which the colours below follow
const themeChange = createSubscriber((update) => {
  const observer = new MutationObserver(update);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "style"]
  });
  return () => observer.disconnect();
});

let pixel: CanvasRenderingContext2D | null = null;

/**
 * A CSS colour, a theme token such as `var(--color-success)` included, as the red, green and blue
 * the page shows it in now, for painting on a canvas. Read again when the theme changes
 */
export const themeRgb = (color: string): Rgb => {
  themeChange();
  const probe = document.createElement("span");
  probe.style.color = color;
  document.body.append(probe);
  const resolved = getComputedStyle(probe).color;
  probe.remove();
  // the computed colour may be in any colour space; a canvas turns it into sRGB
  pixel ??= document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  if (!pixel) return [128, 128, 128];
  pixel.clearRect(0, 0, 1, 1);
  pixel.fillStyle = resolved;
  pixel.fillRect(0, 0, 1, 1);
  const [r, g, b] = pixel.getImageData(0, 0, 1, 1).data;
  return [r, g, b];
};
