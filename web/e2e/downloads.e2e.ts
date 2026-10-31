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
