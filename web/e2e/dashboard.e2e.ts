import { expect, test } from "@playwright/test";

import type { Schemas } from "../src/lib/api/client";
import { summary } from "./support/fakes";
import { visit } from "./support/login";

test("the dashboard shows more at a glance, each figure a link to its list", async ({ page }) => {
  await page.route("**/api/v0/telemetry/reports/transfers/summary**", (route) =>
    route.fulfill({
      json: {
        Download: { Succeeded: summary(40), Errored: summary(3), Cancelled: summary(2) },
        Upload: { Succeeded: summary(25), TimedOut: summary(1) },
        distinctUsers: 9
      } satisfies Schemas["SummaryReport"]
    })
  );
  await visit(page, "/dashboard");
  const now = page.getByRole("region", { name: "Current activity" });
  await expect(now).toContainText("Downloading");
  await expect(now).toContainText("Sharing");
  await expect(page.getByRole("img", { name: "Transfer outcomes" })).toBeVisible();

  // a figure opens the list behind it, filtered
  await page
    .getByRole("link", { name: /3 failed/ })
    .first()
    .click();
  await expect(page).toHaveURL(/#\/downloads\?state=errored$/);
  await expect(page.getByRole("button", { name: /^Failed/ })).toHaveAttribute(
    "aria-pressed",
    "true"
  );
  await page.goto("./#/dashboard");
  await page.getByRole("link", { name: /Users transferred with/ }).click();
  await expect(page).toHaveURL(/#\/users\?has=transfers$/);
  await expect(page.getByRole("button", { name: "Has transfers", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true"
  );
});

test("the activity log docks under the screen, keeps its height and opens with a key", async ({
  page
}) => {
  await visit(page, "/users");
  const area = page.locator("[data-content]");
  const dock = page.getByRole("region", { name: "Activity on the network" });
  const height = () => dock.evaluate((e) => e.getBoundingClientRect().height);
  const before = await area.evaluate((e) => e.clientHeight);

  // Ctrl+J opens it while a field has focus, and the field keeps it
  const field = page.getByRole("combobox", { name: "Search users" });
  await field.fill("car");
  await page.keyboard.press("Control+j");
  await expect(dock).toBeVisible();
  await expect.poll(height).toBe(288);
  await expect(field).toBeFocused();
  // the screen above gives up exactly the panel's height: nothing is covered
  expect(before - (await area.evaluate((e) => e.clientHeight))).toBe(288);

  // its top edge drags it taller, and the height and the open state stay through a reload
  const edge = (await page
    .getByRole("separator", { name: "Height of the activity log" })
    .boundingBox())!;
  await page.mouse.move(edge.x + edge.width / 2, edge.y + edge.height / 2);
  await page.mouse.down();
  await page.mouse.move(edge.x + edge.width / 2, edge.y + edge.height / 2 - 100, { steps: 5 });
  await page.mouse.up();
  await expect.poll(height).toBe(388);
  await page.reload();
  await expect(dock).toBeVisible();
  await expect.poll(height).toBe(388);

  // with reduced motion it opens at its full height at once
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.keyboard.press("Control+j");
  await expect(dock).toBeHidden();
  await page.keyboard.press("Control+j");
  expect(await height()).toBe(388);
});

test("a screen that fails says what went wrong", async ({ page }) => {
  // the same error in the answers for both directions breaks the recent errors' list, keyed by
  // id, which the screen's boundary catches
  await page.route("**/api/v0/telemetry/reports/transfers/exceptions?**", (route) =>
    route.fulfill({
      json: [
        {
          id: "0b3c6f9e-1d1e-4c43-9d55-3a1f0b6e7a10",
          username: "alice",
          direction: "Download",
          filename: "@@a\\Music\\01.flac",
          size: 1,
          bytesTransferred: 0,
          state: "Completed, Errored",
          requestedAt: "2026-10-27T18:00:00Z",
          endedAt: "2026-10-27T18:01:00Z",
          averageSpeed: 0,
          exception: "File not shared."
        }
      ] satisfies Schemas["TransferExceptionDetail"][]
    })
  );
  await visit(page, "/dashboard");
  await page.getByRole("button", { name: "Errors", exact: true }).click();
  const failed = page
    .getByRole("main")
    .getByRole("alert")
    .filter({ hasText: "This screen could not be displayed" });
  await expect(failed.getByRole("button", { name: "Reload" })).toBeVisible();
  // under the caption, the error's own words rather than a stand-in for them
  const detail = failed.locator("p").nth(1);
  await expect(detail).not.toBeEmpty();
  await expect(detail).not.toHaveText(/^(Internal Error|\[object Object\])$/);
});
