import { expect, test } from "@playwright/test";

import { login, textSize } from "./support/login";

test("reads an option with its source and saves a change to the file", async ({ page }) => {
  await login(page, "/system/settings");
  const description = page.locator('[id="soulseek.description"]');
  await expect(description).toHaveValue("A reseek user.");
  const row = page.locator('[id="soulseek.description-help"]').locator("..");
  await expect(row).not.toContainText("config.toml");

  await description.fill("Smoke test profile");
  const soulseek = page.locator("#soulseek");
  await soulseek.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Soulseek: 1 setting saved")).toBeVisible();
  await expect(row).toContainText("config.toml");
  await expect(description).toHaveValue("Smoke test profile");
});

test("finds options by search, follows the section in view and gathers unsaved changes", async ({
  page
}) => {
  await login(page, "/system/settings");
  const side = page.getByRole("navigation", { name: "Settings sections" });
  await expect(side.getByRole("link", { name: "Soulseek" })).toHaveAttribute(
    "aria-current",
    "location"
  );
  // scrolling the page marks the section scrolled to, without the side list being touched
  await page.evaluate(() => document.getElementById("logging")!.scrollIntoView());
  await expect(side.getByRole("link", { name: "Logging" })).toHaveAttribute(
    "aria-current",
    "location"
  );
  await page.evaluate(() => document.getElementById("shares")!.scrollIntoView());
  await expect(side.getByRole("link", { name: "Shares" })).toHaveAttribute(
    "aria-current",
    "location"
  );
  // a click on a category scrolls there, and the section in view is the one marked
  await side.getByRole("link", { name: "Retention" }).click();
  await expect(page.locator("#retention")).toBeInViewport();
  await expect(side.getByRole("link", { name: "Retention" })).toHaveAttribute(
    "aria-current",
    "location"
  );
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/settings.png` });

  // the search keeps only what it finds
  await page.getByRole("searchbox", { name: "Search settings" }).fill("highlight words");
  await expect(page.locator('[id="chat.highlight_words"]')).toBeVisible();
  await expect(page.locator('[id="soulseek.description"]')).toBeHidden();
  await page.getByRole("searchbox", { name: "Search settings" }).fill("");

  // the environment variable and the flag show on hover
  await page
    .getByRole("button", { name: /^Set Listen port from the environment/ })
    .first()
    .hover();
  await expect(page.getByRole("button", { name: /^Copy RESEEK_/ }).first()).toBeVisible();

  // an edit shows in the bar at the bottom, which can discard it
  await page.locator('[id="soulseek.description"]').fill("Edited");
  const bar = page.getByRole("region", { name: "Unsaved changes" });
  await expect(bar).toContainText("1 unsaved change");
  await bar.getByRole("button", { name: "Discard all" }).click();
  await expect(bar).toBeHidden();
});

test("says what auto stands for while the option holds it", async ({ page }) => {
  await login(page, "/system/settings");
  const workers = page.locator('[id="analysis.workers"]');
  const count = page.locator("#analysis").getByText(/^\d+ files? at once$/);
  await expect(workers).toHaveValue("auto");
  await expect(count).toBeVisible();
  await workers.fill("3");
  await expect(count).toBeHidden();
  await workers.fill("AUTO");
  await expect(count).toBeVisible();
});

test("sets how a notification is announced, and offers the permission Desktop needs", async ({
  page
}) => {
  // a permission not decided yet, granted when the browser is asked
  await page.addInitScript(() => {
    Object.defineProperty(Notification, "permission", { value: "default" });
    Notification.requestPermission = async () => "granted";
  });
  await login(page, "/system/settings");
  const section = page.locator("#notifications");
  const notice = section.getByText("Desktop notifications are not turned on in this browser.");
  const choose = (type: string, mode: string) =>
    section.getByRole("radiogroup", { name: type }).getByRole("radio", { name: mode }).click();

  await expect(notice).toBeVisible();
  await expect(
    section
      .getByRole("radiogroup", { name: "Private messages" })
      .getByRole("radio", { name: "Both" })
  ).toBeChecked();

  // the notice follows the choices on the form, saved or not
  await choose("Private messages", "Sound");
  await choose("Mentions", "Off");
  await choose("A batch of downloads finished", "Sound");
  await expect(notice).toBeHidden();
  await choose("An upload was queued", "Desktop");
  await expect(notice).toBeVisible();

  await section.getByRole("button", { name: "Turn on notifications" }).click();
  await expect(notice).toBeHidden();

  // only the upload's mode is saved: the daemon is shared, and the chat page reads the others
  await choose("Private messages", "Both");
  await choose("Mentions", "Both");
  await choose("A batch of downloads finished", "Both");
  await section.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Notifications: 1 setting saved")).toBeVisible();
  await page.reload();
  await expect(
    section.getByRole("radiogroup", { name: "An upload was queued" }).getByRole("radio", {
      name: "Desktop"
    })
  ).toBeChecked();
});

test("a table's Remove button keeps clear of the fields of its row", async ({ page }) => {
  await login(page, "/system/settings");
  const table = page.locator('[id="search.filter_presets"]');
  await table.getByRole("button", { name: "Add to Filter presets" }).click();
  const row = table.locator("tbody tr").last();
  await row.getByRole("textbox").last().fill("a filter long enough to reach the end of its field");
  const remove = row.getByRole("button", { name: /^Remove/ });
  // on a desktop's table, then stacked in a window narrower than a tablet
  for (const width of [1280, 600]) {
    await page.setViewportSize({ width, height: 720 });
    await row.getByRole("textbox").last().focus();
    const button = (await remove.boundingBox())!;
    for (const field of await row.getByRole("textbox").all()) {
      const box = (await field.boundingBox())!;
      const apart =
        button.x >= box.x + box.width ||
        button.x + button.width <= box.x ||
        button.y >= box.y + box.height ||
        button.y + button.height <= box.y;
      expect(apart, `at ${width} px`).toBe(true);
    }
  }
});

test.describe("at a 24 px text size on a 360 px phone", () => {
  test.use({ viewport: { width: 360, height: 640 } });

  test("a notification's four choices stay inside its row", async ({ page }) => {
    await textSize(page, 24);
    await login(page, "/system/settings");
    const section = page.locator("#notifications");
    const group = section.getByRole("radiogroup", { name: "Private messages" });
    await group.scrollIntoViewIfNeeded();
    if (process.env.SHOTS) {
      await section.screenshot({ path: `${process.env.SHOTS}/notification-modes.png` });
    }
    const row = (await section.boundingBox())!;
    const choices = await group.getByRole("radio").all();
    expect(choices).toHaveLength(4);
    for (const choice of choices) {
      await expect(choice).toBeVisible();
      const box = (await choice.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(row.x);
      expect(box.x + box.width).toBeLessThanOrEqual(row.x + row.width);
    }
  });
});
