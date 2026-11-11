import { expect, type Locator, type Page, test } from "@playwright/test";

import { fakeChat } from "./support/fakes";
import { visit } from "./support/login";

type Insets = { top: number; right: number; bottom: number; left: number };

/** Gives the screen safe-area insets, as a phone's notch, corners and browser bar do (Chromium) */
const cover = async (page: Page, insets: Insets) => {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets });
};

/**
 * What a layer holds that reaches into the insets: each child of `layer` whose box crosses an
 * edge of the safe area, as a description. A box of a pixel or less is one kept for screen
 * readers, not one that shows
 */
const outside = (layer: Locator, insets: Insets) =>
  layer.evaluate((root, { top, right, bottom, left }) => {
    const safe = { top, left, right: innerWidth - right, bottom: innerHeight - bottom };
    return [...root.children].flatMap((child) => {
      const box = child.getBoundingClientRect();
      if (box.width <= 1 || box.height <= 1) return [];
      const crosses =
        box.left < safe.left - 0.5 ||
        box.right > safe.right + 0.5 ||
        box.top < safe.top - 0.5 ||
        box.bottom > safe.bottom + 0.5;
      return crosses ? [`${child.tagName.toLowerCase()} at ${JSON.stringify(box)}`] : [];
    });
  }, insets);

test.describe("a phone held sideways, a notch on each side and a bar at the bottom", () => {
  const insets = { top: 0, right: 47, bottom: 21, left: 47 };
  // under the width where the activity log is a sheet over the bottom of the screen
  test.use({ viewport: { width: 740, height: 360 } });

  test("keeps the chats sheet and the activity log inside the safe area", async ({ page }) => {
    await fakeChat(page);
    await visit(page, "/chat/rooms/indie");
    await cover(page, insets);
    await page.getByRole("button", { name: "Chats", exact: true }).click();
    const sheet = page.getByRole("dialog", { name: "Chats" });
    await expect(sheet).toBeVisible();
    // once the sheet has slid in
    await expect.poll(() => outside(sheet, insets)).toEqual([]);
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();

    await page.getByRole("button", { name: /^Activity/ }).click();
    const log = page.getByRole("region", { name: "Activity on the network" });
    await expect(log).toBeVisible();
    await expect.poll(() => outside(log, insets)).toEqual([]);
  });
});

test.describe("a phone held upright, a notch at the top and a bar at the bottom", () => {
  const insets = { top: 59, right: 0, bottom: 34, left: 0 };
  test.use({ viewport: { width: 393, height: 852 } });

  test("keeps the header, the footer and the navigation sheet inside the safe area", async ({
    page
  }) => {
    await visit(page, "/dashboard");
    await cover(page, insets);
    // the shell's own edges, which the sheets, the docked log and the toasts stand on
    await expect.poll(() => outside(page.getByRole("banner"), insets)).toEqual([]);
    await expect.poll(() => outside(page.getByRole("contentinfo"), insets)).toEqual([]);

    await page.getByRole("button", { name: "Toggle navigation" }).click();
    const sheet = page.getByRole("dialog", { name: "Navigation" });
    await expect(sheet).toBeVisible();
    await expect.poll(() => outside(sheet, insets)).toEqual([]);
  });

  test("lays the Add from Downloads sheet along the bottom edge, inside the safe area", async ({
    page
  }) => {
    await visit(page, "/analysis");
    await cover(page, insets);
    await page.getByRole("button", { name: "Add from Downloads" }).first().click();
    const sheet = page.getByRole("dialog", { name: "Add from Downloads" });
    await expect(sheet).toBeVisible();
    // a sheet along the bottom spans the screen's width, its content kept off the bottom inset
    await expect.poll(async () => (await sheet.boundingBox())?.width).toBe(393);
    await expect.poll(() => outside(sheet, insets)).toEqual([]);
  });
});
