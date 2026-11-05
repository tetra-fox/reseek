import { expect, test } from "@playwright/test";

import { transfer } from "./support/fakes";
import { login } from "./support/login";

test("a download's row shows where it is: waiting, moving, then plain once finished", async ({
  page
}) => {
  const files = [
    transfer("@@a\\One\\moving.flac", "InProgress"),
    transfer("@@a\\One\\waiting.flac", "Queued, Remotely"),
    transfer("@@a\\One\\finished.flac")
  ];
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
    (route) =>
      route.fulfill({
        json: [{ username: "alice", directories: [{ directory: "d", fileCount: 3, files }] }]
      })
  );
  await login(page, "/downloads");
  const row = (name: string) => page.getByRole("row").filter({ hasText: name });
  await expect(row("moving.flac").locator("[data-fill]")).toHaveAttribute("data-fill", "moving");
  const width = (bar: Element) => bar.getBoundingClientRect().width;
  const whole = await row("moving.flac").evaluate(width);
  const part = await row("moving.flac").locator("[data-fill] > span").evaluate(width);
  expect(part / whole).toBeCloseTo(0.4, 1);
  await expect(row("waiting.flac").locator("[data-fill]")).toHaveAttribute("data-fill", "waiting");
  await expect(row("finished.flac").locator("[data-fill]")).toHaveCount(0);
});

test("a filter on is named above the list, and Clear filters brings every row back", async ({
  page
}) => {
  const files = [
    transfer("@@a\\One\\moving.flac", "InProgress"),
    transfer("@@a\\One\\waiting.flac", "Queued, Remotely"),
    transfer("@@a\\One\\finished.flac")
  ];
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
    (route) =>
      route.fulfill({
        json: [{ username: "alice", directories: [{ directory: "d", fileCount: 3, files }] }]
      })
  );
  await login(page, "/downloads");
  const table = page.getByRole("treegrid", { name: "Downloads" });
  const row = (name: string) => table.getByRole("row").filter({ hasText: name });
  const names = ["moving.flac", "waiting.flac", "finished.flac"];
  await expect(row("moving.flac")).toBeVisible();

  const queued = page
    .getByRole("group", { name: "State" })
    .getByRole("button", { name: /^Queued/ });
  await queued.click();
  await expect(queued).toHaveAttribute("aria-pressed", "true");
  await expect(row("moving.flac")).toHaveCount(0);
  await expect(page.getByText("Showing 1 of 3 downloads")).toBeVisible();
  await expect(page.getByRole("button", { name: "Remove filter Queued" })).toBeVisible();

  // a filter that hides every row says so where the rows were
  await page.getByRole("searchbox", { name: "Filter" }).fill("nothing-matches");
  await expect(page.getByText("The filters hide all 3 downloads")).toBeVisible();
  await expect(page.getByRole("button", { name: /^Remove filter Matching/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Clear filters" })).toHaveCount(1);

  await page.getByRole("button", { name: "Clear filters" }).click();
  for (const name of names) await expect(row(name)).toBeVisible();
  await expect(page.getByText(/^Showing/)).toHaveCount(0);
  await expect(queued).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("searchbox", { name: "Filter" })).toHaveValue("");
  await expect(table).toBeFocused();
});
