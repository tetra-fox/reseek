import type { Locator } from "@playwright/test";

/**
 * How far two elements overlap from top to bottom, in pixels; 0 or less when one is clear of the
 * other. Null while either has no box
 */
export const overlap = async (a: Locator, b: Locator) => {
  const [one, two] = [await a.boundingBox(), await b.boundingBox()];
  if (!one || !two) return null;
  return Math.min(one.y + one.height, two.y + two.height) - Math.max(one.y, two.y);
};
