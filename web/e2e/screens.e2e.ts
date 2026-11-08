import { expect, test } from "@playwright/test";

import { visit } from "./support/login";

test("opens the dashboard, chat, rooms, a profile and your profile", async ({ page }) => {
  await visit(page, "/dashboard");
  await expect(page.getByRole("radiogroup", { name: "Time range" })).toBeVisible();
  await expect(page.getByText("Users transferred with")).toBeVisible();
  await expect(page.getByRole("img", { name: "Transfers over time" })).toBeVisible();

  await page.getByRole("link", { name: "Chat" }).click();
  await expect(page).toHaveURL(/#\/chat$/);
  await expect(page.getByRole("grid", { name: "Rooms and conversations" })).toBeVisible();
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.getByRole("dialog", { name: "Join room" })).toBeVisible();
  await page.keyboard.press("Escape");

  // a path no screen renders goes where it belongs rather than to a blank page
  await page.goto("./#/nowhere");
  await expect(page).toHaveURL(/#\/dashboard$/);

  await page.goto("./#/users/alice");
  await expect(page.getByRole("heading", { name: "alice" })).toBeVisible();
  await page.getByRole("link", { name: "Transfers" }).last().click();
  await expect(page.getByText("No transfers with this user")).toBeVisible();

  await page.getByRole("button", { name: /Administrator/ }).click();
  await page.getByRole("menuitem", { name: "Your profile" }).click();
  await expect(page).toHaveURL(/#\/profile$/);
  await expect(page.getByText("What others see")).toBeVisible();
  await expect(page.getByLabel("Profile text")).toBeEditable();
});

test("opens the raw settings file and the log history", async ({ page }) => {
  await visit(page, "/system/info");
  await expect(page.getByText("Distributed network")).toBeVisible();
  const tabs = page.getByRole("navigation", { name: "System" });

  await tabs.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("link", { name: "Raw file" }).click();
  await expect(page.getByLabel("Contents of config.toml")).toBeVisible();

  await tabs.getByRole("link", { name: "Logs" }).click();
  await expect(page.getByRole("grid", { name: "Live log" })).toBeVisible();
  await page.getByRole("link", { name: "History" }).click();
  await expect(page).toHaveURL(/tab=history$/);
});
