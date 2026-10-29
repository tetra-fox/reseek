import { expect, test } from "@playwright/test";

import { login } from "./support/login";

test("opens the dashboard, chat, rooms, a profile and your profile", async ({ page }) => {
  await login(page, "/dashboard");
  await expect(page.getByRole("group", { name: "Time range" })).toBeVisible();
  await expect(page.getByText("Users transferred with")).toBeVisible();
  await expect(page.getByRole("img", { name: "Transfers over time" })).toBeVisible();

  await page.getByRole("link", { name: "Chat" }).click();
  await expect(page).toHaveURL(/#\/chat$/);
  await expect(page.getByRole("grid", { name: "Rooms and conversations" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Join a room" })).toBeVisible();
  // the dialog joins public rooms from their rows and creates a room by name, nothing else
  await page.getByRole("button", { name: "Join a room" }).click();
  const dialog = page.getByRole("dialog", { name: "Join a room" });
  await expect(dialog.getByRole("heading", { name: "Public rooms" })).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "Create a room" })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Create", exact: true })).toBeDisabled();
  await dialog.getByLabel("Name").fill("a new room");
  await expect(dialog.getByRole("button", { name: "Create", exact: true })).toBeEnabled();
  await expect(dialog.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  await expect(dialog.getByRole("button", { name: "Join", exact: true })).toHaveCount(0);
  await page.keyboard.press("Escape");

  // rooms are a section of the chat page
  await page.goto("./#/rooms");
  await expect(page).toHaveURL(/#\/chat$/);
  await page.goto("./#/nowhere");
  await expect(page).toHaveURL(/#\/dashboard$/);
  // a path under a screen that renders nothing goes to that screen rather than a blank page
  await page.goto("./#/browse/carol");
  await expect(page).toHaveURL(/#\/browse$/);
  await expect(page.getByLabel("Username")).toBeVisible();

  await page.goto("./#/users/alice");
  await expect(page.getByRole("heading", { name: "alice" })).toBeVisible();
  await page.getByRole("link", { name: "Transfers" }).last().click();
  await expect(page.getByText("No transfers with this user")).toBeVisible();

  await page.getByRole("link", { name: "Your profile" }).click();
  await expect(page).toHaveURL(/#\/profile$/);
  await expect(page.getByText("Public view")).toBeVisible();
  await expect(page.getByLabel("Profile text")).toBeEditable();
});

test("opens the raw settings file and the log history", async ({ page }) => {
  await login(page, "/system/info");
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
