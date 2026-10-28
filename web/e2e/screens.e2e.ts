import { expect, test } from "@playwright/test";

import { login } from "./login";

test("opens search, browse and both transfer screens", async ({ page }) => {
  await login(page, "/searches");
  // the smoke daemon never connects to a Soulseek server
  await expect(page.getByRole("searchbox", { name: "Search for files" })).toBeDisabled();
  await expect(page.getByText("Connect to the Soulseek server to search.")).toBeVisible();
  await expect(page.getByText("No searches to display")).toBeVisible();

  await page.getByRole("link", { name: "Browse" }).click();
  await expect(page).toHaveURL(/#\/browse$/);
  await expect(page.getByLabel("Username")).toBeEnabled();

  await page.getByRole("link", { name: "Downloads" }).click();
  await expect(page.getByText("No downloads to display")).toBeVisible();
  await expect(page.getByRole("radiogroup", { name: "Group by" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Add" })).toBeVisible();

  await page.getByRole("link", { name: "Uploads" }).click();
  await expect(page.getByText("No uploads to display")).toBeVisible();
  await expect(page.getByRole("button", { name: "Add" })).toHaveCount(0);
});
