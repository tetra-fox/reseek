import { expect, test } from "@playwright/test";

import { login } from "./support/login";

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
