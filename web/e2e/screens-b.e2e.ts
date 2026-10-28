import { expect, test } from "@playwright/test";

import { login } from "./login";

test("opens the dashboard, chat, rooms, users, a profile and your profile", async ({ page }) => {
  await login(page, "/dashboard");
  await expect(page.getByRole("group", { name: "Time range" })).toBeVisible();
  await expect(page.getByText("Users transferred with")).toBeVisible();
  await expect(page.getByRole("img", { name: "Transfers over time" })).toBeVisible();

  await page.getByRole("link", { name: "Chat" }).click();
  await expect(page).toHaveURL(/#\/chat$/);
  await expect(page.getByRole("grid", { name: "Rooms and conversations" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Join a room" })).toBeVisible();

  // rooms are a section of the chat page
  await page.goto("./#/rooms");
  await expect(page).toHaveURL(/#\/chat$/);
  await page.goto("./#/nowhere");
  await expect(page).toHaveURL(/#\/dashboard$/);

  await page.getByRole("link", { name: "Users", exact: true }).click();
  await expect(page).toHaveURL(/#\/users$/);
  await expect(page.getByText("No users yet")).toBeVisible();
  await page.getByRole("link", { name: "Blocked users" }).click();
  await expect(page.getByText("No blocked users")).toBeVisible();

  await page.goto("./#/users/alice");
  await expect(page.getByRole("heading", { name: "alice" })).toBeVisible();
  await expect(page.getByText("Not seen before")).toBeVisible();
  await page.getByRole("link", { name: "Transfers" }).last().click();
  await expect(page.getByText("No transfers with this user")).toBeVisible();

  await page.getByRole("link", { name: "Your profile" }).click();
  await expect(page).toHaveURL(/#\/profile$/);
  await expect(page.getByText("How others see you")).toBeVisible();
  await expect(page.getByLabel("Profile text")).toBeEditable();
});

test("opens each System tab", async ({ page }) => {
  await login(page, "/system/info");
  await expect(page.getByText("Distributed network")).toBeVisible();
  const tabs = page.getByRole("navigation", { name: "System" });

  await tabs.getByRole("link", { name: "Settings" }).click();
  await page.getByRole("link", { name: "Raw file" }).click();
  await expect(page.getByLabel("Contents of config.toml")).toBeVisible();

  await tabs.getByRole("link", { name: "Shares" }).click();
  await expect(page.getByText("No shares configured")).toBeVisible();

  await tabs.getByRole("link", { name: "Files" }).click();
  await expect(page.getByRole("link", { name: "/downloads" })).toBeVisible();

  await tabs.getByRole("link", { name: "Data" }).click();
  await expect(page.getByRole("heading", { name: "Finished transfers" })).toBeVisible();

  await tabs.getByRole("link", { name: "Events" }).click();
  await expect(page.getByRole("button", { name: "All types" })).toBeVisible();

  await tabs.getByRole("link", { name: "Logs" }).click();
  await expect(page.getByRole("grid", { name: "Live log" })).toBeVisible();
  await page.getByRole("link", { name: "History" }).click();
  await expect(page).toHaveURL(/tab=history$/);
});
