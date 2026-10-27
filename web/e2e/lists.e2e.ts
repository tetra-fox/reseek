import { expect, type Page, test } from "@playwright/test";

import { login } from "./login";

/** Answers the chat calls with two conversations, `alpha` with `messages` unread ones */
const fakeChat = async (page: Page, messages: number) => {
  const message = (n: number) => ({
    localId: n,
    id: n,
    timestamp: new Date(Date.UTC(2026, 9, 26, 10, 0, n)).toISOString(),
    username: "alpha",
    direction: "In",
    message: `line ${n}\nwith a second line`,
    isAcknowledged: false,
    wasReplayed: false
  });
  const conversation = (username: string, last: number) => ({
    username,
    isActive: true,
    isServerNotice: false,
    unAcknowledgedMessageCount: username === "alpha" ? messages : 0,
    hasUnAcknowledgedMessages: username === "alpha" && messages > 0,
    notify: "all",
    draft: "",
    lastMessage: { ...message(last), username },
    messages: null
  });
  const acknowledged: string[] = [];
  await page.route("**/api/v0/conversations**", async (route) => {
    const url = new URL(route.request().url());
    if (route.request().method() === "PUT") {
      acknowledged.push(url.pathname.replace(/^.*\/api\/v0\//, "/api/v0/"));
      return route.fulfill({ status: 200 });
    }
    if (url.pathname.endsWith("/api/v0/conversations")) {
      return route.fulfill({ json: [conversation("alpha", 2), conversation("beta", 1)] });
    }
    if (url.pathname.endsWith("/messages")) {
      const all = url.pathname.includes("/alpha/")
        ? Array.from({ length: messages }, (_, n) => message(n + 1))
        : [];
      return route.fulfill({ json: all });
    }
    return route.fulfill({ status: 404, json: { error: "not found" } });
  });
  await page.route("**/api/v0/rooms/joined", (route) => route.fulfill({ json: [] }));
  return acknowledged;
};

test("selects a range with Shift-click in a real browser", async ({ page }) => {
  await login(page, "/system/logs");
  const rows = page.getByRole("grid", { name: "Live log" }).getByRole("row");
  await expect(rows.nth(3)).toBeVisible();
  await rows.nth(0).click();
  await rows.nth(2).click({ modifiers: ["Shift"] });
  await expect(page.locator('[role=grid] [aria-selected="true"]')).toHaveCount(3);
  // the press with Shift selected no text on the page
  expect(await page.evaluate(() => document.getSelection()?.toString() ?? "")).toBe("");
  await rows.nth(4).click({ modifiers: ["ControlOrMeta"] });
  await expect(page.locator('[role=grid] [aria-selected="true"]')).toHaveCount(4);
});

test("keeps keyboard focus on the chat list when a click opens a chat", async ({ page }) => {
  await fakeChat(page, 2);
  await login(page, "/chat");
  const grid = page.getByRole("grid", { name: "Rooms and conversations" });
  await grid.locator('[data-line="user:alpha"]').click();
  await expect(page).toHaveURL(/#\/chat\/alpha$/);
  await expect(grid).toBeFocused();
  await page.keyboard.press("ArrowDown");
  const active = await grid.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toHaveAttribute("data-line", "user:beta");
});

test("doesn't acknowledge a conversation opened at an old message", async ({ page }) => {
  const acknowledged = await fakeChat(page, 80);
  await login(page, "/chat");
  await page.goto("./#/chat/alpha?at=3");
  await expect(page.locator('[data-message="3"]')).toBeVisible();
  await page.waitForTimeout(1000);
  expect(acknowledged).toEqual([]);
  // at the bottom, it is read
  await page.getByRole("button", { name: /Jump to latest|new/ }).click();
  await expect.poll(() => acknowledged).toContain("/api/v0/conversations/alpha");
});
