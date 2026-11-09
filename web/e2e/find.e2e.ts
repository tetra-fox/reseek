import { expect, type Page, test } from "@playwright/test";

import { fakeConnected, SEARCH_ID } from "./support/fakes";
import { textSize, visit } from "./support/login";

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
      groups: [],
      uploads: { queue: "default", slots: 10, speed: "default", speedLimit: null, limits: [] },
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
  await visit(page, "/searches");

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

test("the Search box and its dropdown fit a 360 px phone at the largest text size", async ({
  page
}) => {
  await page.setViewportSize({ width: 360, height: 640 });
  await textSize(page, 24);
  await fakeConnected(page);
  await directory(page);
  await visit(page, "/searches");
  // the Search button leaves the box its room
  const button = page.getByRole("button", { name: "Search", exact: true });
  await expect(button).toBeVisible();
  const { width } = (await button.boundingBox())!;
  expect(width).toBeLessThanOrEqual(24 * 2.75);
  expect((await box(page).boundingBox())!.width).toBeGreaterThan(24 * 7);

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
