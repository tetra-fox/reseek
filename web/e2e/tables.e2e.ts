import { expect, type Locator, test } from "@playwright/test";

import type { Schemas } from "../src/lib/api/client";
import { carolShares, dir, transfer, user } from "./support/fakes";
import { visit } from "./support/login";

// every table keeps its columns where its header puts them, whatever one row holds: a name far
// wider than the window, with nowhere to break, moves no other cell of its row

/** A name far wider than any window, with nowhere to break */
const LONG = `${"a_very_long_name_".repeat(24)}end`;

/**
 * Expects every row of `grid` with a cell per column as wide as its header row, each cell starting
 * where the header starts its column. A tree's folder lines, drawn otherwise, are left out, and so
 * are its first `indented` cells, which a line starts further in the deeper it is
 */
const expectAligned = async (grid: Locator, indented = 0) => {
  await expect(grid.getByRole("row").nth(2)).toBeVisible();
  const rows = await grid.evaluate((box) =>
    [...box.querySelectorAll('[role="row"]')].map((row) => ({
      width: Math.round(row.getBoundingClientRect().width),
      starts: [
        ...row.querySelectorAll(':scope > [role="columnheader"], :scope > [role="gridcell"]')
      ].map((cell) => Math.round(cell.getBoundingClientRect().left))
    }))
  );
  const [header, ...lines] = rows;
  const cells = lines.filter((line) => line.starts.length === header.starts.length);
  expect(cells.length).toBeGreaterThan(1);
  const fixed = (row: typeof header) => ({ ...row, starts: row.starts.slice(indented) });
  for (const line of cells) expect(fixed(line)).toEqual(fixed(header));
};

test("a user's transfers", async ({ page }) => {
  const transfers = [
    transfer("@@a\\Music\\1.flac"),
    transfer(`@@a\\${LONG}\\${LONG}.flac`, "InProgress")
  ];
  await page.route(
    (url) => url.pathname.includes("/api/v0/users/alice"),
    (route) =>
      new URL(route.request().url()).pathname.endsWith("/transfers")
        ? route.fulfill({ json: { transfers, total: transfers.length } })
        : route.fulfill({ json: user("alice") })
  );
  await visit(page, "/users/alice/transfers");
  await expectAligned(page.getByRole("grid", { name: "Transfers with alice" }));
});

test("the shares", async ({ page }) => {
  const share = (localPath: string, alias: string): Schemas["Share"] => ({
    id: localPath,
    localPath,
    alias,
    raw: `[${alias}]${localPath}`,
    remotePath: `@@${alias}`,
    isExcluded: false,
    visibility: "everyone",
    directories: 12,
    files: 140
  });
  await page.route("**/api/v0/shares", (route) =>
    route.request().method() === "GET"
      ? route.fulfill({
          json: { local: [share("/srv/music", "music"), share(`/srv/${LONG}`, LONG)] }
        })
      : route.fallback()
  );
  await visit(page, "/system/shares");
  await expectAligned(page.getByRole("grid", { name: "Shared folders" }));
});

test("a browse of someone's shares", async ({ page }) => {
  await carolShares(page, [dir("@@c\\Music", ["1.flac", `${LONG}.flac`])]);
  await visit(page, "/users/carol/shares");
  await page.locator('[data-line$="Music"]').click();
  // the tick and the name
  await expectAligned(page.getByRole("treegrid", { name: "Shared folders" }), 2);
});
