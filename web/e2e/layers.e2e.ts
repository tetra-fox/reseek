import { expect, type Page, test } from "@playwright/test";

import { fakeChat, fakeDownloads } from "./support/fakes";
import { login } from "./support/login";

/** Whether the header leaves `box` whole: it ends above it, and the box is what shows at its top */
const clearOfHeader = (page: Page, selector: string) =>
  page.evaluate((selector) => {
    const box = document.querySelector(selector)!.getBoundingClientRect();
    const header = document.querySelector("header")!.getBoundingClientRect();
    const top = document.elementFromPoint(box.left + box.width / 2, box.top + 4);
    return { under: box.top >= header.bottom - 0.5, shown: !!top?.closest(selector) };
  }, selector);

test.describe("at 1280 px", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the inspector opens under the header, never beneath it", async ({ page }) => {
    await fakeDownloads(page);
    await login(page, "/downloads");
    await page.getByRole("button", { name: "2.flac" }).first().click();
    const pane = "[data-inspector]";
    await expect(page.locator(pane)).toBeVisible();
    await expect.poll(() => clearOfHeader(page, pane)).toEqual({ under: true, shown: true });
  });
});

test.describe("at 390 px", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("the inspector is a sheet over the whole screen, the header under it", async ({ page }) => {
    await fakeDownloads(page);
    await login(page, "/downloads");
    await page.getByRole("button", { name: "2.flac" }).first().click();
    const sheet = page.locator("[data-inspector]");
    await expect(sheet).toBeVisible();
    await expect.poll(() => sheet.boundingBox()).toEqual({ x: 0, y: 0, width: 390, height: 844 });
    const onTop = await page.evaluate(() => {
      const top = document.elementFromPoint(100, 10);
      return !!top?.closest("[data-inspector]");
    });
    expect(onTop).toBe(true);
  });
});

for (const [width, height] of [
  [1280, 800],
  [390, 844]
]) {
  test.describe(`at ${width} px`, () => {
    test.use({ viewport: { width, height } });

    test("the chat fills the content area, edge to edge", async ({ page }) => {
      await fakeChat(page);
      await login(page, "/chat/rooms/indie");
      await expect(page.getByText("anyone have the new boards of canada rip?")).toBeVisible();
      const gutter = await page.evaluate(() => {
        const area = document.querySelector("[data-content]")!.getBoundingClientRect();
        const chat = document.querySelector("[data-chat]")!.getBoundingClientRect();
        return [
          chat.left - area.left,
          area.right - chat.right,
          chat.top - area.top,
          area.bottom - chat.bottom
        ].map(Math.round);
      });
      expect(gutter, "the gutter left, right, above and below").toEqual([0, 0, 0, 0]);
    });
  });
}
