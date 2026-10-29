import { expect, test } from "@playwright/test";

import { login } from "./support/login";

const counts = { succeeded: 2, failed: 0, active: 0, queued: 0, bytes: 0 };
const alice = {
  username: "alice",
  known: true,
  firstSeenAt: "2026-10-01T10:00:00Z",
  lastSeenAt: "2026-10-27T10:00:00Z",
  presence: "Offline",
  presenceAt: "2026-10-27T09:00:00Z",
  isPrivileged: false,
  statistics: null,
  info: null,
  countryCode: "NL",
  group: { name: "default", source: "builtin", priority: 1, color: null },
  groupOverride: null,
  block: null,
  notes: null,
  description: null,
  tags: ["jazz"],
  muted: false,
  pinned: false,
  conversation: null,
  endpoint: null,
  figures: { downloads: counts, uploads: counts, messages: { in: 1, out: 1, unread: 0 } },
  unreadMessages: 0,
  activeTransfers: 0
};

test("a user's card and row offer Message, Browse and Add buddy", async ({ page }) => {
  let pinned = false;
  await page.route("**/api/v0/users?**", (route) =>
    route.fulfill({ json: { users: [{ ...alice, pinned }], total: 1 } })
  );
  await page.route("**/api/v0/users/alice/**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/pin")) {
      pinned = route.request().method() === "PUT";
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ json: { ...alice, pinned } });
  });
  await login(page, "/users");
  const row = page.locator('[data-line="alice"]');
  await expect(row).toBeVisible();
  // the card on hover, with the actions as buttons and when they were last seen
  await row.getByRole("link", { name: "alice", exact: true }).hover();
  const card = page.getByRole("dialog", { name: "Profile of alice" });
  await expect(card).toBeVisible();
  await expect(card).toContainText("last seen");
  await expect(card.getByRole("link", { name: "Message" })).toBeVisible();
  await expect(card.getByRole("link", { name: "Browse" })).toBeVisible();
  await card.getByRole("button", { name: "Add buddy" }).click();
  await expect(card.getByRole("button", { name: "Remove buddy" })).toBeVisible();
  await page.mouse.move(5, 5);
  // the row has the same action
  await expect(row.getByRole("button", { name: /buddy/ })).toBeVisible();
});

test("notes save as you type and say so with a tick", async ({ page }) => {
  let saved = "";
  await page.route("**/api/v0/users/alice/**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/notes")) {
      saved = route.request().postDataJSON().notes;
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ json: { ...alice, notes: saved, figures: alice.figures } });
  });
  await login(page, "/users/alice");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("alice");
  await page.getByLabel("Notes").fill("shares rare jazz");
  await expect(page.getByRole("status").filter({ hasText: "Saved" })).toBeVisible();
  expect(saved).toBe("shares rare jazz");
});
