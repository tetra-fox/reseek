import { expect, test } from "@playwright/test";

import { visit } from "./support/login";

test("runs the cleanup from its schedule and finds it in the list by its kind", async ({
  page
}) => {
  await visit(page, "/jobs");
  await expect(page.getByRole("heading", { name: "Scheduled jobs" })).toBeVisible();
  const cleanup = page.getByRole("listitem").filter({ hasText: "Deletes old data" });
  await expect(cleanup).toContainText("Every 5 minutes");
  // an off schedule says so and where to turn it on
  const vacuum = page.getByRole("listitem").filter({ hasText: "Database vacuum" });
  await expect(vacuum).toContainText("Off");
  await expect(vacuum.getByRole("link", { name: "Turn on in Settings" })).toBeVisible();

  await cleanup.getByRole("button", { name: "Run now" }).click();
  const row = page.locator("li[id^=job-]").filter({ hasText: "Started by you" }).first();
  await expect(row).toContainText("Cleanup");
  await expect(row).toContainText("Succeeded");
  await row.getByRole("button", { name: "Show parts of Cleanup" }).click();
  await expect(row).toContainText("Finished transfers");
  await expect(row).toContainText("Sessions and tokens");

  await page.getByRole("button", { name: "Kind" }).click();
  await page.getByRole("menuitemcheckbox", { name: "Maintenance" }).click();
  await page.keyboard.press("Escape");
  await expect(row).toBeVisible();
});
