import { expect, test } from "@playwright/test";

import { login } from "./login";

const summary = (count: number) => ({
  count,
  totalBytes: count * 40_000_000,
  distinctUsers: 3,
  averageSpeed: 900_000,
  averageWait: 4,
  averageDuration: 30
});

test("the dashboard shows more at a glance, each figure a link to its list", async ({ page }) => {
  await page.route("**/api/v0/telemetry/reports/transfers/summary**", (route) =>
    route.fulfill({
      json: {
        Download: { Succeeded: summary(40), Errored: summary(3), Cancelled: summary(2) },
        Upload: { Succeeded: summary(25), TimedOut: summary(1) },
        distinctUsers: 9
      }
    })
  );
  await login(page, "/dashboard");
  const now = page.getByRole("region", { name: "Current activity" });
  await expect(now).toContainText("Downloading");
  await expect(now).toContainText("Sharing");
  await expect(page.getByRole("img", { name: "Transfer outcomes" })).toBeVisible();
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/dashboard.png` });

  // a figure opens the list behind it, filtered
  await page
    .getByRole("link", { name: /3 failed/ })
    .first()
    .click();
  await expect(page).toHaveURL(/#\/downloads\?state=errored$/);
  await expect(page.getByRole("button", { name: /^Errored/ })).toHaveAttribute(
    "aria-pressed",
    "true"
  );
  await page.goto("./#/dashboard");
  await page.getByRole("link", { name: /Users transferred with/ }).click();
  await expect(page).toHaveURL(/#\/users\?has=transfers$/);
  await expect(page.getByRole("button", { name: "Has transfers" })).toHaveAttribute(
    "aria-pressed",
    "true"
  );
});

test("the activity log opens from the footer and lists what others did", async ({ page }) => {
  await page.route("**/api/v0/activity", (route) =>
    route.fulfill({
      json: [
        { id: 1, at: "2026-10-27T18:00:00Z", username: "alice", kind: { type: "browse" } },
        {
          id: 2,
          at: "2026-10-27T18:01:00Z",
          username: "bob",
          kind: { type: "search", query: "boards of canada", results: 12 }
        },
        {
          id: 3,
          at: "2026-10-27T18:02:00Z",
          username: "carol",
          kind: { type: "queued", files: 3, filename: "Music\\Album\\03.flac" }
        }
      ]
    })
  );
  await login(page, "/dashboard");
  await page.getByRole("button", { name: "Activity" }).click();
  const log = page.getByRole("grid", { name: "Activity" });
  await expect(log.getByRole("row")).toHaveCount(3);
  await expect(log.getByRole("row").first()).toContainText("carol queued 3 files");
  await expect(log).toContainText('bob searched for "boards of canada" and got 12 results');
  await expect(log).toContainText("alice is browsing your shares");
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/activity.png` });
  await page.getByRole("button", { name: "Close the activity" }).click();
  await expect(log).toBeHidden();
});
