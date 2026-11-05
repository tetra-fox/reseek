import { expect, type Locator, type Page, test } from "@playwright/test";

import { carolShares, dir, fakeConnected, transfer } from "./support/fakes";
import { login } from "./support/login";

const MUSIC = "@@c\\Music";
const tracks = (prefix: string, n: number) =>
  Array.from({ length: n }, (_, k) => `${prefix}${String(k + 1).padStart(2, "0")}.flac`);

/**
 * Answers a browse of carol's shares: Music with Albums of thirty tracks, Bootlegs of three and
 * Deep\CD1 of forty, and a locked folder
 */
const fakeShare = (page: Page) =>
  carolShares(
    page,
    [
      dir(MUSIC, []),
      dir(`${MUSIC}\\Albums`, tracks("A", 30)),
      dir(`${MUSIC}\\Bootlegs`, tracks("B", 3)),
      dir(`${MUSIC}\\Deep`, []),
      dir(`${MUSIC}\\Deep\\CD1`, tracks("D", 40))
    ],
    [dir(`${MUSIC}\\Locked`, ["secret.flac"])]
  );

/** A line by its key, its backslashes escaped for the selector */
const line = (tree: Locator, key: string) =>
  tree.locator(`[data-line="${key.replaceAll("\\", "\\\\")}"]`);

/** The line of a folder of carol's music, or of one of its files */
const folder = (tree: Locator, path: string) => line(tree, `folder:${MUSIC}${path}`);
const fileLine = (tree: Locator, path: string) => line(tree, `${MUSIC}${path}`);

/** Opens carol's Shares tab, the tree drawn */
const browseCarol = async (page: Page, opened?: string) => {
  await fakeShare(page);
  await login(page, "/users/carol/shares");
  if (opened) await page.goto(`./#/users/carol/shares?path=${encodeURIComponent(opened)}`);
  const tree = page.getByRole("treegrid", { name: "Shared folders" });
  await expect(folder(tree, "")).toBeVisible();
  return tree;
};

/** Whether the element plays the animation `name` now */
const animates = (locator: Locator, name: string) =>
  locator.evaluate(
    (e, wanted) => e.getAnimations().some((a) => (a as CSSAnimation).animationName === wanted),
    name
  );

test("a folder opens in place with its files under it, and they come in", async ({ page }) => {
  const tree = await browseCarol(page);
  const albums = folder(tree, "\\Albums");
  await expect(fileLine(tree, "\\Albums\\A01.flac")).toHaveCount(0);
  await albums.click();
  const first = fileLine(tree, "\\Albums\\A01.flac");
  await expect(first).toBeVisible();
  expect(await animates(first, "line-in")).toBe(true);
  // the folder's files come right after its line, before the next folder's
  const keys = await tree
    .locator("[role=rowgroup] [data-line]")
    .evaluateAll((lines) => lines.map((l) => l.getAttribute("data-line")));
  const at = keys.indexOf(`folder:${MUSIC}\\Albums`);
  expect(keys[at + 1]).toBe(`${MUSIC}\\Albums\\A01.flac`);
  // and closes again from its line
  await albums.click();
  await expect(first).toHaveCount(0);
});

test("Shift-click ticks a range across two folders, and the bar counts it", async ({ page }) => {
  const tree = await browseCarol(page);
  await folder(tree, "\\Bootlegs").click();
  await folder(tree, "\\Albums").click();
  const tick = (path: string) => fileLine(tree, path).getByRole("checkbox");
  // the end of Albums and the start of Bootlegs, at the foot of the tree
  await tree.evaluate((box) => (box.scrollTop = box.scrollHeight));
  await tick("\\Albums\\A29.flac").click();
  await tick("\\Bootlegs\\B02.flac").click({ modifiers: ["Shift"] });
  // A29, A30, then Bootlegs' B01 and B02: the folder line between is no row of its own
  const bar = page.getByRole("toolbar", { name: "Selected files" });
  await expect(bar).toContainText("4 files");
  await expect(folder(tree, "\\Bootlegs").getByRole("checkbox")).toHaveAttribute(
    "data-state",
    "indeterminate"
  );
});

test("the folders a deep line is in stay pinned while it scrolls", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 700 });
  const tree = await browseCarol(page);
  await folder(tree, "\\Deep").click();
  await folder(tree, "\\Deep\\CD1").click();
  // D30 is the 35th line: Music, Albums, Bootlegs, Deep, CD1, then its tracks
  const row = await folder(tree, "").evaluate((l) => l.getBoundingClientRect().height);
  await tree.evaluate((box, y) => (box.scrollTop = y), 34 * row - 150);
  const pinned = () =>
    tree.locator("[data-pinned]").evaluateAll((pins) =>
      pins
        .filter((pin) => {
          const r = pin.getBoundingClientRect();
          const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
          return !!hit && pin.contains(hit);
        })
        .sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top)
        .map((pin) => pin.getAttribute("data-line"))
    );
  await expect
    .poll(pinned)
    .toEqual([`folder:${MUSIC}`, `folder:${MUSIC}\\Deep`, `folder:${MUSIC}\\Deep\\CD1`]);
});

test("a locked folder is dimmed and nothing in it can be ticked", async ({ page }) => {
  const tree = await browseCarol(page);
  const locked = folder(tree, "\\Locked");
  await expect(locked.getByTitle(/shares them only with buddies/)).toBeVisible();
  await expect(locked.getByRole("checkbox")).toHaveCount(0);
  await expect(locked.getByRole("button", { name: /Download this folder/ })).toHaveCount(0);
  await locked.click();
  await expect(fileLine(tree, "\\Locked\\secret.flac")).toBeVisible();
  await expect(fileLine(tree, "\\Locked\\secret.flac").getByRole("checkbox")).toHaveCount(0);
});

test("the filter shows the way to each match, open", async ({ page }) => {
  const tree = await browseCarol(page);
  await page.getByLabel("Filter", { exact: true }).fill("D07");
  await expect(fileLine(tree, "\\Deep\\CD1\\D07.flac")).toBeVisible();
  await expect(folder(tree, "\\Deep")).toBeVisible();
  await expect(folder(tree, "\\Albums")).toHaveCount(0);
  await expect(fileLine(tree, "\\Deep\\CD1\\D08.flac")).toHaveCount(0);
});

test("a link to a folder opens the tree down to it and goes to its line", async ({ page }) => {
  const tree = await browseCarol(page, `${MUSIC}\\Deep\\CD1`);
  await expect(fileLine(tree, "\\Deep\\CD1\\D01.flac")).toBeVisible();
  const active = await tree.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toHaveAttribute(
    "data-line",
    `folder:${MUSIC}\\Deep\\CD1`
  );
  const crumbs = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(crumbs).toContainText("CD1");
});

test("a file's row fills as it downloads, then flashes and shows it done", async ({ page }) => {
  await fakeConnected(page);
  // the daemon's list of downloads, which the test moves along
  let downloads: ReturnType<typeof transfer>[] = [];
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
    (route) =>
      route.fulfill({
        json: downloads.length
          ? [
              {
                username: "carol",
                directories: [{ directory: "d", fileCount: 1, files: downloads }]
              }
            ]
          : []
      })
  );
  await page.route("**/api/v0/transfers/downloads/jobs", (route) =>
    route.fulfill({
      json: { id: "j", batches: [{ username: "carol", batchId: "b", failures: [], error: null }] }
    })
  );
  const tree = await browseCarol(page);
  await folder(tree, "\\Bootlegs").click();
  const row = fileLine(tree, "\\Bootlegs\\B01.flac");
  await row.getByRole("button", { name: "Actions for B01.flac" }).click();
  await page.getByRole("menuitem", { name: "Download" }).click();
  // at once, before the daemon lists it: waiting
  await expect(row.locator("[data-fill]")).toHaveAttribute("data-fill", "waiting");
  await expect(row).toContainText("Queued");

  /** Lists the download in `state`, `percent` done, and has the screen read the list again */
  const move = async (state: string, percent: number) => {
    const t = transfer(`${MUSIC}\\Bootlegs\\B01.flac`, state, 1000, "carol");
    downloads = [{ ...t, id: "d1", percentComplete: percent, bytesTransferred: percent * 10 }];
    await page.mouse.move(0, 0);
    await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  };
  const width = () =>
    row.locator("[data-fill] > span").evaluate((bar) => bar.getBoundingClientRect().width);
  await move("InProgress", 30);
  await expect(row.locator("[data-fill]")).toHaveAttribute("data-fill", "moving");
  await expect.poll(width).toBeGreaterThan(0);
  const before = await width();
  await move("InProgress", 80);
  await expect.poll(width).toBeGreaterThan(before * 2);
  await move("Completed, Succeeded", 100);
  await expect(row.locator("[data-fill]")).toHaveAttribute("data-fill", "flash");
  await expect(row.locator("[data-fill]")).toHaveCount(0);
  await expect(row.getByLabel("Downloaded")).toBeVisible();
});
