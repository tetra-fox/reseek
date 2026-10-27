import { expect, type Locator, type Page, test } from "@playwright/test";

import { login } from "./login";

/**
 * Answers the chat calls with two conversations, `alpha` with `messages` unread ones, a page at
 * a time; an older page takes `slow` milliseconds
 */
const fakeChat = async (page: Page, messages: number, slow = 0) => {
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
      if (!url.pathname.includes("/alpha/")) return route.fulfill({ json: [] });
      const before = Number(url.searchParams.get("before") ?? messages + 1);
      const limit = Number(url.searchParams.get("limit") ?? 100);
      const first = Math.max(1, before - limit);
      if (url.searchParams.has("before")) await new Promise((done) => setTimeout(done, slow));
      return route.fulfill({
        json: Array.from({ length: before - first }, (_, n) => message(before - 1 - n))
      });
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

/**
 * From the next frame on, notes in every frame where the first message in view then is drawn,
 * relative to the log's top
 */
const watchTop = (log: Locator) =>
  log.evaluate(
    (box) =>
      new Promise<void>((started) =>
        requestAnimationFrame(() => {
          const view = box.getBoundingClientRect().top;
          const first = [...box.querySelectorAll<HTMLElement>("[data-message]")].find(
            (node) => node.getBoundingClientRect().bottom > view
          )!;
          const id = first.dataset.message!;
          const frames: number[] = [];
          Object.assign(window, { anchored: frames });
          const note = () => {
            const node = box.querySelector(`[data-message="${id}"]`);
            frames.push(node ? Math.round(node.getBoundingClientRect().top - view) : NaN);
            if (frames.length < 90) requestAnimationFrame(note);
          };
          note();
          started();
        })
      )
  );

/** The places `watchTop` noted, once the older page is in and settled */
const watched = async (page: Page, older: string) => {
  await expect(page.locator(`[data-message="${older}"]`)).toBeAttached();
  await page.waitForTimeout(1000);
  return page.evaluate(() => (window as unknown as { anchored: number[] }).anchored);
};

test("keeps the message in view still while an older page loads above it", async ({ page }) => {
  await fakeChat(page, 250, 400);
  await login(page, "/chat/alpha");
  const log = page.getByRole("log");
  await expect(page.locator('[data-message="250"]')).toBeVisible();
  // scroll up a step at a time, as a wheel would, until the older page is asked for
  for (let i = 0; i < 40; i++) {
    await log.evaluate((box) => box.scrollBy(0, -400));
    await page.waitForTimeout(30);
    if (await log.evaluate((box) => box.scrollTop < 96)) break;
  }
  await watchTop(log);
  const frames = await watched(page, "150");
  expect(new Set(frames).size, `drawn at ${frames.join(", ")}`).toBe(1);
});

test("keeps the message in view still after a jump to the top loads older ones", async ({
  page
}) => {
  await fakeChat(page, 250, 400);
  await login(page, "/chat/alpha");
  const log = page.getByRole("log");
  await expect(page.locator('[data-message="250"]')).toBeVisible();
  // one step past everything drawn, as dragging the scrollbar to the top would
  await log.evaluate((box) => (box.scrollTop = 0));
  await watchTop(log);
  const first = await watched(page, "150");
  expect(new Set(first).size, `first page drawn at ${first.join(", ")}`).toBe(1);
  await log.evaluate((box) => (box.scrollTop = 0));
  await watchTop(log);
  const second = await watched(page, "50");
  expect(new Set(second).size, `second page drawn at ${second.join(", ")}`).toBe(1);
});
