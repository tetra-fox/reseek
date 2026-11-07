import { expect, type Locator, type Page } from "@playwright/test";

/** A plot's place in the canvas, in CSS pixels, as the canvas writes it */
export const rect = async (plot: Locator, which = "data-plot") => {
  const [x, y, width, height] = (await plot.getAttribute(which))!.split(" ").map(Number);
  return { x, y, width, height };
};

/** A locator's box once two reads 100 ms apart agree, as a dialog settles into place */
export const settled = async (locator: Locator) => {
  // the poll's first try is at once, so the first box is read there and compared 100 ms on
  let last: Awaited<ReturnType<Locator["boundingBox"]>> | undefined;
  await expect
    .poll(
      async () => {
        const now = await locator.boundingBox();
        const still =
          !!now && !!last && now.x === last.x && now.y === last.y && now.width === last.width;
        last = now;
        return still;
      },
      { message: "the box keeps moving", intervals: [100] }
    )
    .toBe(true);
  // the poll passed only on a box read twice
  return last!;
};

/** Opens a download on the Analysis page from its row, and gives its slot */
export const openSlot = async (page: Page) => {
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Open in Analysis" }).click();
  return page.getByRole("region", { name: "Slot A" });
};
