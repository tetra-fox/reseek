import { expect, test } from "@playwright/test";

import { fakeDownloadList, transfer, userDownloads } from "./support/fakes";
import { visit } from "./support/login";

test("a download's row shows where it is: waiting, moving, then plain once finished", async ({
  page
}) => {
  const files = [
    transfer("@@a\\One\\moving.flac", "InProgress"),
    transfer("@@a\\One\\waiting.flac", "Queued, Remotely"),
    transfer("@@a\\One\\finished.flac")
  ];
  await fakeDownloadList(page, () => [userDownloads("alice", files)]);
  await visit(page, "/downloads");
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
  await fakeDownloadList(page, () => [userDownloads("alice", files)]);
  await visit(page, "/downloads");
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
  await expect(page.getByRole("button", { name: "Remove filter Queued" })).toBeVisible();

  // a filter that hides every row has its chip and one Clear filters
  await page.getByRole("searchbox", { name: "Filter" }).fill("nothing-matches");
  await expect(page.getByRole("button", { name: /^Remove filter Matching/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Clear filters" })).toHaveCount(1);

  await page.getByRole("button", { name: "Clear filters" }).click();
  for (const name of names) await expect(row(name)).toBeVisible();
  await expect(queued).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("searchbox", { name: "Filter" })).toHaveValue("");
  await expect(table).toBeFocused();
});

test("a right-click on a download offers what its ... button does, and on its user the group's", async ({
  page
}) => {
  const files = [transfer("@@a\\One\\finished.flac")];
  await fakeDownloadList(page, () => [userDownloads("alice", files)]);
  await visit(page, "/downloads");
  const table = page.getByRole("treegrid", { name: "Downloads" });
  const row = table.getByRole("row").filter({ hasText: "finished.flac" });
  await row.getByRole("button", { name: "More for finished.flac" }).click();
  const offered = await page.getByRole("menuitem").allTextContents();
  expect(offered).toContain("Remove");
  await page.keyboard.press("Escape");
  await row.getByText("finished.flac").click({ button: "right" });
  await expect(page.getByRole("menuitem")).toHaveText(offered);
  await page.keyboard.press("Escape");

  await table.getByRole("row").filter({ hasText: "alice" }).first().click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Remove group" })).toBeVisible();
});

test("a line keeps its tint and its buttons while the pointer is on its right-click menu", async ({
  page
}) => {
  await fakeDownloadList(page, () => [userDownloads("alice", [transfer("@@a\\One\\a.flac")])]);
  await visit(page, "/downloads");
  const table = page.getByRole("treegrid", { name: "Downloads" });
  const row = table.getByRole("row").filter({ hasText: "a.flac" });
  await row.getByText("a.flac").click({ button: "right" });
  await page.getByRole("menuitem").first().hover();
  await expect(row).not.toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
  await expect(row.locator(".row-action")).toHaveCSS("opacity", "1");
});
