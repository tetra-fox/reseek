import { expect, test } from "@playwright/test";

import { launchOptions } from "./support/browser";
import { conversation, privateMessage } from "./support/fakes";
import { open } from "./support/login";

// headless chromium hides the scrollbars, which leaves none to press; drawn, a press on one is
// the browser's own
test.use({ launchOptions: { ...launchOptions, ignoreDefaultArgs: ["--hide-scrollbars"] } });

test("a conversation held at the top by its scrollbar reads page after page to the start", async ({
  page
}) => {
  const total = 250;
  // the message each older page was read before, and the page's size
  const asked: string[] = [];
  let size = 0;
  await page.route("**/api/v0/conversations**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/api/v0/conversations")) {
      return route.fulfill({ json: [conversation("alice")] });
    }
    if (!url.pathname.endsWith("/alice/messages")) {
      return route.fulfill({ json: conversation("alice") });
    }
    const older = url.searchParams.get("before");
    if (older !== null) asked.push(older);
    size = Number(url.searchParams.get("limit"));
    const before = Number(url.searchParams.get("before") ?? total + 1);
    const first = Math.max(1, before - size);
    const at = (id: number) => new Date(Date.UTC(2026, 9, 27, 0, 0, id)).toISOString();
    return route.fulfill({
      json: Array.from({ length: before - first }, (_, n) =>
        privateMessage(before - 1 - n, at(before - 1 - n), "alice", "In", `line ${before - 1 - n}`)
      )
    });
  });
  await open(page, "/chat/alice");
  const log = page.getByRole("log", { name: "Messages" });
  await expect(log.getByText(`line ${total}`, { exact: true })).toBeVisible();
  const width = await log.evaluate((box: HTMLElement) => box.offsetWidth - box.clientWidth);
  expect(width, "the scrollbar is drawn").toBeGreaterThan(0);

  // the thumb is at the bottom, at the newest message: pressed there, it is dragged past the
  // top and held
  const box = (await log.boundingBox())!;
  const x = box.x + box.width - width / 2;
  await page.mouse.move(x, box.y + box.height - width * 2);
  await page.mouse.down();
  await page.mouse.move(x, box.y - 100, { steps: 20 });
  await expect(log.getByText("line 1", { exact: true })).toBeAttached();
  expect(asked).toEqual([String(total + 1 - size), String(total + 1 - 2 * size)]);
  await page.mouse.up();
});
