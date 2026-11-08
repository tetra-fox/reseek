import { expect, type Locator, type Page, test } from "@playwright/test";

import type { Schemas } from "../src/lib/api/client";
import { BUDDY } from "../src/lib/wording";
import { carolShares, dir, transfer, user } from "./support/fakes";
import { visit } from "./support/login";

// every table keeps its columns where its header puts them, whatever one row holds: a name far
// wider than the window, with nowhere to break, moves no other cell of its row

/** A name far wider than any window, with nowhere to break */
const LONG = `${"a_very_long_name_".repeat(24)}end`;
const AT = "2026-10-27T10:00:00Z";

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

/** Answers the users list, the directory's and the buddies', with `users` */
const users = (page: Page, list: Schemas["UserSummary"][]) =>
  page.route("**/api/v0/users?**", (route) =>
    route.fulfill({ json: { users: list, total: list.length } })
  );

test("the users directory", async ({ page }) => {
  await users(page, [user("alice"), user(LONG, { tags: [LONG] })]);
  await visit(page, "/users");
  await expectAligned(page.getByRole("grid", { name: "Users" }));
});

test("the buddy list", async ({ page }) => {
  const buddy = { trusted: false, notifyPresence: true };
  await users(page, [user("alice", { buddy }), user(LONG, { buddy, notes: LONG })]);
  await visit(page, "/users?tab=buddies");
  await expectAligned(page.getByRole("grid", { name: BUDDY.plural }));
});

test("the blocked users", async ({ page }) => {
  const block = (username: string, reason: string): Schemas["Block"] => ({
    username,
    reason,
    scopes: ["uploads", "searches", "browse", "messages"],
    source: "api",
    addresses: [],
    createdAt: AT
  });
  await page.route("**/api/v0/users/blocked", (route) =>
    route.fulfill({
      json: {
        users: [block("alice", "spam"), { ...block(LONG, LONG), addresses: ["203.0.113.7"] }],
        rules: { patterns: [], cidrs: [] }
      } satisfies Schemas["BlockedUsers"]
    })
  );
  await visit(page, "/users?tab=blocked");
  await expectAligned(page.getByRole("grid", { name: "Blocked users" }));
});

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

test("the event log", async ({ page }) => {
  const event = (id: string, path: string) => ({
    id,
    timestamp: AT,
    type: "DownloadFileComplete",
    data: { username: "alice", localPath: path }
  });
  await page.route("**/api/v0/events?**", (route) =>
    route.fulfill({
      json: [event("e1", "/downloads/1.flac"), event("e2", `/downloads/${LONG}`)],
      headers: { "X-Total-Count": "2" }
    })
  );
  await visit(page, "/system/events");
  await expectAligned(page.getByRole("grid", { name: "Events" }));
});

test("the log files", async ({ page }) => {
  const file = (name: string): Schemas["FilesystemFile"] => ({
    name,
    fullName: name,
    length: 4096,
    createdAt: AT,
    modifiedAt: AT,
    attributes: "Normal"
  });
  await page.route("**/api/v0/logs/files", (route) =>
    route.fulfill({ json: [file("reseek-20261027.log"), file(`${LONG}.log`)] })
  );
  await visit(page, "/system/logs?tab=history");
  await expectAligned(page.getByRole("grid", { name: "Log files" }));
});

test("a browse of someone's shares", async ({ page }) => {
  await carolShares(page, [dir("@@c\\Music", ["1.flac", `${LONG}.flac`])]);
  await visit(page, "/users/carol/shares");
  await page.locator('[data-line$="Music"]').click();
  // the tick and the name
  await expectAligned(page.getByRole("treegrid", { name: "Shared folders" }), 2);
});
