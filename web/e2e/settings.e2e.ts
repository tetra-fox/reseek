import { expect, test } from "@playwright/test";

import { login } from "./login";

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
  // a click on a category scrolls there and marks it
  await side.getByRole("link", { name: "Retention" }).click();
  await expect(side.getByRole("link", { name: "Retention" })).toHaveAttribute(
    "aria-current",
    "location"
  );
  await expect(page.locator("#retention")).toBeInViewport();
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
