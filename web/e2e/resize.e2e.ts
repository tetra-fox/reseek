import { expect, type Locator, type Page, test } from "@playwright/test";

import { fakeChat, fakeConnected } from "./support/fakes";
import { login } from "./support/login";

/** Drags the handle sideways by `by` pixels */
const drag = async (page: Page, handle: Locator, by: number) => {
  const box = (await handle.boundingBox())!;
  const y = box.y + box.height / 2;
  const x = box.x + box.width / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + by, y, { steps: 4 });
  await page.mouse.up();
};

const width = (handle: Locator) => handle.getAttribute("aria-valuenow").then(Number);

/** The room's chat, the pane its handles resize */
const room = async (page: Page) => {
  await fakeConnected(page);
  await fakeChat(page);
  await login(page, "/chat/rooms/indie");
};

test.describe("on a wide screen", () => {
  test.use({ viewport: { width: 1600, height: 900 } });

  for (const { label, sign, initial, min } of [
    { label: "Width of the chat list", sign: 1, initial: 288, min: 240 },
    { label: "Width of the side panel", sign: -1, initial: 256, min: 208 }
  ]) {
    test(`${label}: a drag, the keys, limits, a reset and a reload`, async ({ page }) => {
      await room(page);
      const handle = page.getByRole("separator", { name: label });
      await expect(handle).toHaveAttribute("aria-orientation", "vertical");
      expect(await width(handle)).toBe(initial);

      await drag(page, handle, sign * 60);
      await expect.poll(() => width(handle)).toBe(initial + 60);
      await expect(handle.locator("..")).toHaveJSProperty("offsetWidth", initial + 60);

      // no further than its limits, wherever the pointer goes
      await drag(page, handle, -sign * 900);
      await expect.poll(() => width(handle)).toBe(min);
      await drag(page, handle, sign * 900);
      await expect
        .poll(() => handle.getAttribute("aria-valuemax"))
        .toBe(String(await width(handle)));

      // the keys: a step, and the least with Home
      await handle.press("Home");
      await expect.poll(() => width(handle)).toBe(min);
      await handle.press(sign > 0 ? "ArrowRight" : "ArrowLeft");
      await expect.poll(() => width(handle)).toBe(min + 20);

      await page.reload();
      await expect.poll(() => width(handle)).toBe(min + 20);

      await handle.dblclick();
      await expect.poll(() => width(handle)).toBe(initial);
    });
  }

  test("the messages keep their room as a narrow window limits the columns", async ({ page }) => {
    await room(page);
    const list = page.getByRole("separator", { name: "Width of the chat list" });
    const side = page.getByRole("separator", { name: "Width of the side panel" });
    await drag(page, list, 900);
    await drag(page, side, -900);
    await page.setViewportSize({ width: 1100, height: 800 });
    const total = (await page.locator("[data-chat]").boundingBox())!.width;
    await expect
      .poll(async () => total - (await width(list)) - (await width(side)))
      .toBeGreaterThanOrEqual(320);
  });
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("nothing is resized: the lists are sheets", async ({ page }) => {
    await fakeChat(page);
    await login(page, "/chat/rooms/indie");
    await expect(page.getByRole("heading", { name: "indie" })).toBeVisible();
    await expect(page.getByRole("separator")).toHaveCount(0);
  });
});
