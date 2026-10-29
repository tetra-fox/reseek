import { expect, type Locator, test } from "@playwright/test";

import { fakeFiles, fakeSearch, file, SEARCH_ID } from "./fakes";
import { login, textSize } from "./login";

/** How far apart two names start across the page, in pixels */
const apart = async (a: Locator, b: Locator) => {
  const [x, y] = await Promise.all([a.boundingBox(), b.boundingBox()]);
  return Math.abs((x?.x ?? NaN) - (y?.x ?? NaN));
};

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  for (const root of [16, 24]) {
    test.describe(`at ${width} px, a ${root} px root`, () => {
      test.use({ viewport: { width, height } });

      test("a file's name starts where its sibling folder's does", async ({ page }) => {
        await textSize(page, root);
        await fakeSearch(page);
        await fakeFiles(page);
        // a file beside a folder: Album holds a track and the Bonus folder
        await page.route(`**/api/v0/searches/${SEARCH_ID}/responses**`, (route) =>
          route.fulfill({
            json: [
              {
                username: "alice",
                country: "NL",
                token: 1,
                hasFreeUploadSlot: true,
                uploadSpeed: 3_000_000,
                queueLength: 0,
                fileCount: 2,
                files: [
                  file("@@a\\Music\\Album\\01 One.flac"),
                  file("@@a\\Music\\Album\\Bonus\\05 Demo.flac")
                ],
                lockedFileCount: 0,
                lockedFiles: []
              }
            ]
          })
        );

        await login(page, "/system/files");
        const files = page.getByRole("treegrid");
        const folder = files.getByRole("button", { name: "In Rainbows", exact: true });
        const note = files.getByText("notes.txt", { exact: true });
        await expect(note).toBeVisible();
        expect(await apart(folder, note), "Files").toBeLessThan(0.5);

        await page.goto(`./#/searches/${SEARCH_ID}`);
        const card = page.locator('section[data-key="0:alice"]');
        const bonus = card.getByTitle("@@a\\Music\\Album\\Bonus", { exact: true });
        const track = card.getByText("01 One.flac", { exact: true });
        await expect(track).toBeVisible();
        expect(await apart(bonus, track), "search results").toBeLessThan(0.5);
      });
    });
  }
}
