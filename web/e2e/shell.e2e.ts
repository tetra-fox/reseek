import { expect, test } from "@playwright/test";

test("logs in, shows the connection live and moves between screens", async ({ page }) => {
  await page.goto("./#/downloads");
  await expect(page).toHaveURL(/#\/login\?to=%2Fdownloads$/);
  await expect(page.getByLabel("Username")).toBeFocused();
  await page.getByLabel("Username").fill("reseek");
  await page.getByLabel("Password").fill("wrong");
  await page.getByRole("button", { name: "Login" }).click();
  await expect(page.getByRole("alert")).toHaveText("Invalid username or password");
  await expect(page.getByLabel("Username")).toBeFocused();

  await page.getByLabel("Password").fill("reseek");
  await page.getByRole("button", { name: "Login" }).click();
  await expect(page).toHaveURL(/#\/downloads$/);
  await expect(page.getByText("No downloads to display")).toBeVisible();
  await expect(page.getByRole("status", { name: "Updates: Live" })).toBeVisible();
  await expect(page.locator("footer")).toContainText("Disconnected");

  await page.getByRole("link", { name: "Search" }).click();
  await expect(page).toHaveURL(/#\/searches$/);
  await expect(page).toHaveTitle("Search - reseek");

  await page.getByRole("link", { name: "System" }).click();
  await expect(page).toHaveURL(/#\/system\/info$/);
});
