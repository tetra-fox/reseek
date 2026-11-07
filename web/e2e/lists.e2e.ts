import { expect, type Locator, type Page, test } from "@playwright/test";

import { conversation, privateMessage } from "./support/fakes";
import { visit } from "./support/login";

/**
 * Answers the chat calls with two conversations, `alpha` with `messages` unread ones, a page at
 * a time; an older page takes `slow` milliseconds
 */
const fakeUnread = async (page: Page, messages: number, slow = 0) => {
  const message = (n: number, username = "alpha") =>
    privateMessage(
      n,
      new Date(Date.UTC(2026, 9, 26, 10, 0, n)).toISOString(),
      username,
      "In",
      `line ${n}\nwith a second line`,
      { isAcknowledged: false }
    );
  const unread = (username: string, last: number) =>
    conversation(username, {
      unAcknowledgedMessageCount: username === "alpha" ? messages : 0,
      hasUnAcknowledgedMessages: username === "alpha" && messages > 0,
      lastMessage: message(last, username)
    });
  await page.route("**/api/v0/conversations**", async (route) => {
    const url = new URL(route.request().url());
    // an acknowledgment ends here: the daemon has none of these conversations
    if (route.request().method() === "PUT") return route.fulfill({ status: 200 });
    if (url.pathname.endsWith("/api/v0/conversations")) {
      return route.fulfill({ json: [unread("alpha", 2), unread("beta", 1)] });
    }
    if (url.pathname.endsWith("/messages")) {
      if (!url.pathname.includes("/alpha/")) return route.fulfill({ json: [] });
      const limit = Number(url.searchParams.get("limit") ?? 100);
      const after = url.searchParams.get("after");
      if (after !== null) {
        const from = Number(after) + 1;
        const count = Math.min(limit, Math.max(0, messages - from + 1));
        return route.fulfill({ json: Array.from({ length: count }, (_, n) => message(from + n)) });
      }
      const before = Number(url.searchParams.get("before") ?? messages + 1);
      const first = Math.max(1, before - limit);
      if (url.searchParams.has("before")) await new Promise((done) => setTimeout(done, slow));
      return route.fulfill({
        json: Array.from({ length: before - first }, (_, n) => message(before - 1 - n))
      });
    }
    return route.fulfill({ status: 404, json: { error: "not found" } });
  });
  await page.route("**/api/v0/rooms/joined", (route) => route.fulfill({ json: [] }));
};

test("selects a range with Shift-click in a real browser", async ({ page }) => {
  await visit(page, "/system/logs");
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
  await fakeUnread(page, 2);
  await visit(page, "/chat");
  const grid = page.getByRole("grid", { name: "Rooms and conversations" });
  await grid.locator('[data-line="user:alpha"]').click();
  await expect(page).toHaveURL(/#\/chat\/alpha$/);
  await expect(grid).toBeFocused();
  await page.keyboard.press("ArrowDown");
  const active = await grid.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toHaveAttribute("data-line", "user:beta");
});

/**
 * From the next frame on, notes in every frame where the first message in view then is painted,
 * relative to the log's top. A frame's own callback runs before the list measures the lines it
 * just drew and moves them, so each place is read in a task after that frame's paint
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
          let count = 0;
          Object.assign(window, { anchored: frames });
          const note = () => {
            setTimeout(() => {
              const node = box.querySelector(`[data-message="${id}"]`);
              frames.push(node ? Math.round(node.getBoundingClientRect().top - view) : NaN);
            }, 0);
            if (++count < 90) requestAnimationFrame(note);
          };
          note();
          started();
        })
      )
  );

/** The places `watchTop` noted in its 90 frames, once the older page is in */
const watched = async (page: Page, older: string) => {
  await expect(page.locator(`[data-message="${older}"]`)).toBeAttached();
  await page.waitForFunction(
    () => (window as unknown as { anchored: number[] }).anchored.length >= 90
  );
  return page.evaluate(() => (window as unknown as { anchored: number[] }).anchored);
};

test("keeps the message in view still while an older page loads above it", async ({ page }) => {
  await fakeUnread(page, 250, 400);
  await visit(page, "/chat/alpha");
  const log = page.getByRole("log");
  await expect(page.locator('[data-message="250"]')).toBeVisible();
  // scroll up a step a frame, as a wheel would, until the older page is asked for
  await log.evaluate(
    (box) =>
      new Promise<void>((done) => {
        let steps = 0;
        const step = () => {
          box.scrollBy(0, -400);
          steps += 1;
          if (box.scrollTop < 96 || steps === 40) done();
          else requestAnimationFrame(step);
        };
        step();
      })
  );
  await watchTop(log);
  const frames = await watched(page, "150");
  expect(new Set(frames).size, `drawn at ${frames.join(", ")}`).toBe(1);
});

test("keeps the message in view still after a jump to the top loads older ones", async ({
  page
}) => {
  await fakeUnread(page, 250, 400);
  await visit(page, "/chat/alpha");
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
