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
