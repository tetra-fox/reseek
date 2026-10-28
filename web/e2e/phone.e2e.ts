import { expect, type Page, test } from "@playwright/test";

import { fakeChat, fakeReports, fakeSearch, SEARCH_ID } from "./fakes";
import { login } from "./login";

// a phone's width: no screen may be wider than it, so nothing runs off its right edge
test.use({ viewport: { width: 390, height: 844 } });

/**
 * Waits for `ready`, then checks the content area holds the screen: no sideways scroll, and
 * nothing cut off at its right edge by a box that hides what overflows it. A list that scrolls
 * sideways on its own is fine
 */
const holds = async (page: Page, what: string, ready: (page: Page) => Promise<void>) => {
  await ready(page);
  const [scroll, client] = await page
    .locator("[data-content]")
    .evaluate((e) => [e.scrollWidth, e.clientWidth]);
  expect(scroll, `${what} is ${scroll - client}px wider than a phone`).toBeLessThanOrEqual(client);
  const cut = await page.locator("[data-content]").evaluate((area) => {
    const edge = area.getBoundingClientRect().right + 0.5;
    const scrolls = (e: Element) => ["auto", "scroll"].includes(getComputedStyle(e).overflowX);
    for (const e of area.querySelectorAll("main *")) {
      const box = e.getBoundingClientRect();
      if (!box.width || box.right <= edge || getComputedStyle(e).visibility === "hidden") continue;
      let inside = false;
      for (let a = e.parentElement; a && a !== area; a = a.parentElement) {
        if (scrolls(a)) inside = true;
      }
      if (!inside) return `${e.tagName.toLowerCase()} "${e.textContent?.trim().slice(0, 40)}"`;
    }
    return null;
  });
  expect(cut, `${what} cuts off ${cut}`).toBeNull();
};

/** Opens `path` and checks it fits */
const fits = async (page: Page, path: string, ready: (page: Page) => Promise<void>) => {
  await page.goto(`./#${path}`);
  await holds(page, path, ready);
};

const shows = (text: string | RegExp) => async (page: Page) =>
  expect(page.locator("main").getByText(text).first()).toBeVisible();

test("every screen fits a phone's width", async ({ page }) => {
  await fakeReports(page);
  await fakeSearch(page);
  await fakeChat(page);
  await login(page, "/dashboard");

  await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
  for (const tab of ["Folders", "Errors"]) {
    await page.getByRole("button", { name: tab, exact: true }).click();
    await holds(
      page,
      `the dashboard's ${tab} tab`,
      shows(tab === "Folders" ? "Most uploaded folders" : "Recent errors")
    );
  }
  await fits(page, "/searches", shows("No searches yet"));
  await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
  await fits(page, "/downloads", shows("No downloads yet"));
  await fits(page, "/uploads", shows("No uploads yet"));
  await fits(page, "/chat", shows("indie"));
  await fits(page, "/chat/rooms/indie", shows("anyone have the new boards of canada rip?"));
  await fits(page, "/chat/alice", shows("yes, browse away"));
  await fits(page, "/users", shows("No users yet"));
  await fits(page, "/users?tab=blocked", shows("No blocked users"));
  await fits(page, "/users/alice", shows("Not seen before"));
  await fits(page, "/browse", shows("Browse a user's shares"));
  await fits(page, "/profile", shows("How others see you"));
  for (const [tab, text] of [
    ["info", "Process ID"],
    ["settings", "Listen port"],
    ["account", "API tokens"],
    ["shares", "No shares configured"],
    ["files", "/downloads"],
    ["data", "Finished transfers"],
    ["events", /types/],
    ["logs", "Live"]
  ] as const) {
    await fits(page, `/system/${tab}`, shows(text));
  }
});
