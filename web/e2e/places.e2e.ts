import { expect, type Locator, type Page, test } from "@playwright/test";

import type { Schemas } from "../src/lib/api/client";
import { fakeDownloadList, transfer, user, userDownloads } from "./support/fakes";
import { visit } from "./support/login";

/** Goes to another screen and back by the navigation, as someone switching screens does */
const away = async (page: Page, back: string) => {
  const nav = page.getByRole("navigation", { name: "Screens" });
  await nav.getByRole("link", { name: "Dashboard" }).click();
  await expect(page).toHaveURL(/#\/dashboard$/);
  return () => nav.getByRole("link", { name: back }).click();
};

/**
 * Follows a link to `route` from the page in view, in the running app, as one in a menu or a
 * notification is followed: the app keeps what it has read
 */
const follow = async (page: Page, route: string) => {
  await page.evaluate((to) => {
    const link = document.createElement("a");
    link.href = `#${to}`;
    link.textContent = "Follow";
    link.id = "follow";
    document.querySelector("main")!.append(link);
  }, route);
  await page.locator("#follow").click();
};

/** Answers the users directory with `names`, read again on every call, whatever the filter */
const fakeDirectory = (page: Page, names: () => string[]) =>
  page.route("**/api/v0/users?**", (route) => {
    const url = new URL(route.request().url());
    const offset = Number(url.searchParams.get("offset") ?? 0);
    const limit = Number(url.searchParams.get("limit") ?? 50);
    const all = names().map((name) => user(name));
    return route.fulfill({ json: { users: all.slice(offset, offset + limit), total: all.length } });
  });

/** The row at the top of `box` and how far its top is below the box's */
const topRow = (box: Locator) =>
  box.evaluate((el) => {
    const top = el.getBoundingClientRect().top + 40;
    const rows = [...el.querySelectorAll<HTMLElement>("[role=row][data-line]")];
    const row = rows.find((r) => r.getBoundingClientRect().bottom > top && !r.dataset.pinned)!;
    return { key: row.dataset.line, offset: Math.round(row.getBoundingClientRect().top) };
  });

test("a list comes back on the row it was left at, where rows arrived above it", async ({
  page
}) => {
  const name = (n: number) => `@@a\\One\\${n + 100}.flac`;
  let files = Array.from({ length: 80 }, (_, n) => transfer(name(n * 2)));
  await fakeDownloadList(page, () => [userDownloads("alice", files)]);
  await visit(page, "/downloads");
  const table = page.getByRole("treegrid", { name: "Downloads" });
  await expect(table.getByRole("row").filter({ hasText: "100.flac" })).toBeVisible();
  await table.hover();
  await page.mouse.wheel(0, 900);
  await expect.poll(() => table.evaluate((el) => el.scrollTop)).toBeGreaterThan(800);
  const left = await topRow(table);

  const back = await away(page, "Downloads");
  // rows that sort before every one shown came while the screen was away
  files = [...Array.from({ length: 5 }, (_, n) => transfer(name(n * 2 - 11))), ...files];
  const read = page.waitForResponse((r) => r.url().endsWith("/api/v0/transfers/downloads"));
  await back();
  await read;
  await expect.poll(() => topRow(table)).toEqual(left);
  // and it stays there once the list has taken in the rows read
  await page.waitForTimeout(500);
  expect(await topRow(table)).toEqual(left);
});

test("a page left at its top opens at its top, over rows that came meanwhile", async ({ page }) => {
  let names = Array.from({ length: 50 }, (_, n) => `user${n + 100}`);
  await fakeDirectory(page, () => names);
  await visit(page, "/users");
  const content = page.locator("[data-content]");
  await expect(page.locator('[data-line="user100"]')).toBeVisible();
  await content.hover();
  await page.mouse.wheel(0, 900);
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBeGreaterThan(800);
  await page.mouse.wheel(0, -2000);
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBe(0);
  const back = await away(page, "Users");
  names = [...Array.from({ length: 6 }, (_, n) => `new${n}`), ...names];
  await back();
  await expect(page.locator('[data-line="new0"]')).toBeVisible();
  await expect(page.locator('[data-line="user100"]')).toBeVisible();
  expect(await content.evaluate((el) => el.scrollTop)).toBe(0);
});

test("a page scrolled down comes back there, and a link under another filter opens at the top", async ({
  page
}) => {
  // a page without rows, kept by its offset
  await visit(page, "/system/settings");
  const content = page.locator("[data-content]");
  await expect(page.getByRole("searchbox", { name: "Search settings" })).toBeVisible();
  await content.hover();
  await page.mouse.wheel(0, 1200);
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBeGreaterThan(1000);
  const scrolled = await content.evaluate((el) => el.scrollTop);
  const back = await away(page, "System");
  await back();
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBe(scrolled);

  // the directory left scrolled down, then a count of users transferred with followed
  await fakeDirectory(page, () => Array.from({ length: 50 }, (_, n) => `user${n + 100}`));
  await page
    .getByRole("navigation", { name: "Screens" })
    .getByRole("link", { name: "Users" })
    .click();
  await expect(page.locator('[data-line="user100"]')).toBeVisible();
  await content.hover();
  await page.mouse.wheel(0, 900);
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBeGreaterThan(800);
  await away(page, "Users");
  await follow(page, "/users?has=transfers");
  await expect(page.getByRole("button", { name: "Has transfers", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true"
  );
  await expect(page.locator('[data-line="user100"]')).toBeInViewport();
  expect(await content.evaluate((el) => el.scrollTop)).toBe(0);
});

/** A finished cleanup job, the `n`th of the evening */
const job = (n: number) =>
  ({
    id: `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`,
    kind: "maintenance",
    name: `Cleanup ${n}`,
    starter: { kind: "schedule", name: "cleanup" },
    state: "ended",
    ending: "succeeded",
    counts: { total: 1, succeeded: 1, failed: 0, canceled: 0, paused: 0 },
    pausable: false,
    createdAt: new Date(Date.UTC(2026, 9, 27, 18, 0, 60 - n)).toISOString(),
    startedAt: null,
    endedAt: null,
    error: null,
    follows: null,
    progress: null,
    summary: null
  }) satisfies Schemas["Job"];

test("a link to a job shows it, and Jobs comes back from the navigation where it was left", async ({
  page
}) => {
  const jobs = Array.from({ length: 50 }, (_, n) => job(n));
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/jobs"),
    (route) => route.fulfill({ json: { jobs } satisfies Schemas["JobList"] })
  );
  await visit(page, "/jobs");
  const content = page.locator("[data-content]");
  await expect(page.getByText("Cleanup 0", { exact: true })).toBeVisible();
  await content.hover();
  await page.mouse.wheel(0, 150);
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBeGreaterThan(100);
  await away(page, "Jobs");

  // a link in the running app, with the jobs already read, as Show job in a menu is
  await follow(page, `/jobs?job=${jobs[25].id}`);
  const row = page.locator(`#job-${jobs[25].id}`);
  await expect(row).toBeInViewport();
  // the link points once and leaves the address
  await expect(page).toHaveURL(/#\/jobs$/);
  // and the row stays in view once a return to the old place would be over
  await page.waitForTimeout(2500);
  await expect(row).toBeInViewport();

  // read on, then away and back by the navigation: Jobs is where it was left
  const read = await content.evaluate((el) => el.scrollTop);
  await content.hover();
  await page.mouse.wheel(0, 100);
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBeGreaterThan(read);
  const left = await content.evaluate((el) => el.scrollTop);
  const back = await away(page, "Jobs");
  await back();
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBe(left);
});

test("a System path that is no tab goes to the tab System was left at, once", async ({ page }) => {
  await visit(page, "/system/logs");
  let moves = 0;
  await page.exposeFunction("moved", () => moves++);
  await page.evaluate(() =>
    window.addEventListener("hashchange", () =>
      (window as unknown as { moved: () => void }).moved()
    )
  );
  await page.evaluate(() => (location.hash = "#/system/nonsense"));
  await expect(page).toHaveURL(/#\/system\/logs$/);
  await page.waitForTimeout(500);
  expect(moves).toBeLessThan(5);
  await expect(page).toHaveURL(/#\/system\/logs$/);
  const system = page
    .getByRole("navigation", { name: "Screens" })
    .getByRole("link", { name: "System" });
  await expect(system).toHaveAttribute("href", "#/system/logs");
});
