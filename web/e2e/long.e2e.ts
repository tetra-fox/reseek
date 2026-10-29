import { expect, type Locator, type Page, test } from "@playwright/test";

import { longRoom, manyDownloads, manyResults, SEARCH_ID, wideBrowse } from "./support/fakes";
import { login } from "./support/login";

/** Scrolls `box` to its end a step at a time, as a held key or a fling would */
const toEnd = (box: Locator) =>
  box.evaluate(
    (e) =>
      new Promise<void>((done) => {
        const step = () => {
          e.scrollTop += 3 * e.clientHeight;
          if (e.scrollTop + e.clientHeight >= e.scrollHeight - 1) setTimeout(done, 300);
          else requestAnimationFrame(step);
        };
        step();
      })
  );

/** How many lines `box` holds in the page now */
const drawn = (box: Locator, line: string) => box.locator(line).count();

/** Checks a long list draws a window of its lines, before and after a scroll to its end */
const windowed = async (page: Page, box: Locator, line: string, last: string) => {
  await expect(box.locator(line).first()).toBeVisible();
  expect(await drawn(box, line), "lines drawn at the start").toBeLessThan(150);
  await toEnd(box);
  await expect(box.getByText(last)).toBeVisible();
  expect(await drawn(box, line), "lines drawn at the end").toBeLessThan(150);
};

test("1000 downloads draw a window and scroll to the last", async ({ page }) => {
  await manyDownloads(page, 1000);
  await page.addInitScript(() =>
    localStorage.setItem("transfers.view.download", JSON.stringify({ groupBy: "flat" }))
  );
  await login(page, "/downloads");
  const grid = page.getByRole("grid", { name: "Downloads" });
  await windowed(page, grid, "[role=rowgroup] [role=row]", "00999 Track.flac");
});

test("focus and a range of rows go past the window's edge", async ({ page }) => {
  await manyDownloads(page, 1000);
  await page.addInitScript(() =>
    localStorage.setItem("transfers.view.download", JSON.stringify({ groupBy: "flat" }))
  );
  await login(page, "/downloads");
  const grid = page.getByRole("grid", { name: "Downloads" });
  await grid.locator('[data-line="00000000-0000-4000-8000-000000000000"]').click({
    position: { x: 2, y: 5 }
  });
  // the keys walk the focus far past the rows first drawn, which follow it into view
  for (let i = 0; i < 80; i++) await page.keyboard.press("ArrowDown");
  const focused = grid.locator('[data-line="00000000-0000-4000-8000-000000000080"]');
  await expect(focused).toBeInViewport();
  await expect(grid).toHaveAttribute("aria-activedescendant", (await focused.getAttribute("id"))!);
  // a Shift-click there selects every row from the first, drawn or not
  await focused.click({ modifiers: ["Shift"], position: { x: 2, y: 5 } });
  await expect(page.getByText("81 selected")).toBeVisible();
});

test("1000 search results in the dense view draw a window and scroll to the last", async ({
  page
}) => {
  await manyResults(page, 1000);
  await login(page, `/searches/${SEARCH_ID}`);
  await page.getByRole("radio", { name: "Dense" }).click();
  const grid = page.getByRole("grid", { name: "Search results" });
  // the results are ranked fastest user first, so the slowest user's last file ends the list
  await windowed(page, grid, "[role=rowgroup] [role=row]", "00009 Song.flac");
});

test("a browse of 5000 folders draws a window and scrolls to the last", async ({ page }) => {
  await wideBrowse(page, 5000);
  await login(page, "/browse");
  await page.getByLabel("Username").fill("carol");
  await page.getByLabel("Username").press("Enter");
  const tree = page.getByRole("treegrid", { name: "Shared folders" });
  await expect(tree.locator("[role=rowgroup] [role=row]").first()).toBeVisible();
  await page.getByRole("button", { name: "Expand all" }).click();
  // the last artist's last album, with its one file: one folder for the shares, then fifty lines
  // to each artist
  await windowed(page, tree, "[role=rowgroup] [role=row]", "Album 47");
  await expect(tree.locator("[role=rowgroup] [role=row]").last()).toContainText("1.flac");
});

test("a room's 1000 messages draw a window and scroll back to the first", async ({ page }) => {
  await longRoom(page, 1000);
  await login(page, "/chat/rooms/indie");
  const log = page.getByRole("log");
  // the last of them is one of the longer messages
  await expect(log.getByText("a longer message, number 999,")).toBeVisible();
  expect(await drawn(log, "[data-message]"), "messages drawn at the end").toBeLessThan(150);
  await log.evaluate(
    (e) =>
      new Promise<void>((done) => {
        const step = () => {
          e.scrollTop -= 3 * e.clientHeight;
          if (e.scrollTop <= 0) setTimeout(done, 300);
          else requestAnimationFrame(step);
        };
        step();
      })
  );
  await expect(log.getByText("message number 1", { exact: true })).toBeVisible();
  expect(await drawn(log, "[data-message]"), "messages drawn at the start").toBeLessThan(150);
});
