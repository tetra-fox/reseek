import { expect, type Page, test } from "@playwright/test";

import { fakeConnected, SEARCH_ID } from "./support/fakes";
import { login, textSize } from "./support/login";

const box = (page: Page) => page.getByRole("combobox", { name: "Search files and users" });
const options = (page: Page) => page.getByRole("option");

/** Answers the directory with a user named like what is searched for */
const directory = (page: Page) =>
  page.route("**/api/v0/users?**", (route) => {
    const q = new URL(route.request().url()).searchParams.get("q") ?? "";
    const user = {
      username: `${q}_fan`,
      known: true,
      presence: "Online",
      isPrivileged: false,
      group: { name: "default", source: "builtin", priority: 1, color: null },
      tags: [],
      muted: false,
      buddy: null,
      lastSeenAt: null,
      figures: {
        uploads: { succeeded: 0, failed: 0, active: 0, queued: 0 },
        downloads: { succeeded: 0, failed: 0, active: 0, queued: 0 },
        messages: { in: 0, out: 0, unread: 0 }
      },
      unreadMessages: 0,
      activeTransfers: 0
    };
    return route.fulfill({ json: { users: [user], total: 1 } });
  });

test("Enter in the Search box always searches, only a chosen row does anything else", async ({
  page
}) => {
  await fakeConnected(page);
  await directory(page);
  // this user is made up: a browse reaching the daemon the tests share would put them in its
  // directory, where a later test finds them
  await page.route("**/api/v0/users/aphex_fan/**", (route) =>
    route.fulfill({ status: 404, json: { error: "No such user" } })
  );
  const posted: string[] = [];
  await page.route("**/api/v0/searches", (route) => {
    if (route.request().method() !== "POST") return route.fallback();
    const body = route.request().postDataJSON() as { id: string; searchText: string };
    posted.push(body.searchText);
    return route.fulfill({ json: { ...body, id: SEARCH_ID } });
  });
  await login(page, "/searches");

  await box(page).fill("aphex");
  await expect(options(page)).toHaveText([
    /Search for aphex/,
    /\baphex_fan\b.*Online/,
    /Look up aphex/
  ]);
  // a mouse left over a row changes nothing
  await options(page).nth(1).hover();
  await box(page).press("Enter");
  await expect.poll(() => posted).toEqual(["aphex"]);
  await expect(page).toHaveURL(/#\/searches\/[^/]+$/);

  await page.goto("./#/searches");
  await box(page).fill("aphex");
  await expect(options(page)).toHaveCount(3);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/users\/aphex_fan\/shares$/);
  expect(posted).toEqual(["aphex"]);
});

test("the Search dropdown fits a phone at the largest text size", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 640 });
  await textSize(page, 24);
  await fakeConnected(page);
  await directory(page);
  await login(page, "/searches");
  await box(page).fill("a-name-as-long-as-they-can-be-0");
  const list = page.getByRole("listbox");
  await expect(list).toBeVisible();
  const wide = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth
  );
  expect(wide).toBe(0);
  const edge = await list.evaluate((e) => e.getBoundingClientRect().right);
  expect(edge).toBeLessThanOrEqual(360);
});
