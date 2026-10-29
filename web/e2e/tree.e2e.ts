import { expect, type Locator, test } from "@playwright/test";

import { dir, fakeFiles, fakeSearch, file, SEARCH_ID, transfer } from "./support/fakes";
import { login, textSize } from "./support/login";

/** How far apart two names start across the page, in pixels */
const apart = async (a: Locator, b: Locator) => {
  const [x, y] = await Promise.all([a.boundingBox(), b.boundingBox()]);
  return Math.abs((x?.x ?? NaN) - (y?.x ?? NaN));
};

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  // at a 16 px root rem and px are the same length, so only a larger root shows a mix of them
  test.describe(`at ${width} px, a 24 px root`, () => {
    test.use({ viewport: { width, height } });

    test("a file's name starts where its sibling folder's does", async ({ page }) => {
      await textSize(page, 24);
      await fakeSearch(page);
      await fakeFiles(page);
      // a file beside a folder, in search results and in the downloads: Album holds a track and
      // the Bonus folder
      await page.route(
        (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
        (route) =>
          route.fulfill({
            json: [
              {
                username: "alice",
                directories: [
                  {
                    directory: "d",
                    fileCount: 2,
                    files: [
                      transfer("@@a\\Music\\Album\\01 One.flac"),
                      transfer("@@a\\Music\\Album\\Bonus\\05 Demo.flac")
                    ]
                  }
                ]
              }
            ]
          })
      );
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
      const results = page.getByRole("treegrid", { name: "Search results" });
      const bonus = results
        .locator("[role=rowgroup]")
        .getByTitle("@@a\\Music\\Album\\Bonus", { exact: true });
      const track = results.getByText("01 One.flac", { exact: true });
      await expect(track).toBeVisible();
      expect(await apart(bonus, track), "search results").toBeLessThan(0.5);

      // a browse, where Music holds a file beside its Albums folder
      await page.route(
        (url) => url.pathname.endsWith("/api/v0/users/carol/browse"),
        (route) =>
          route.fulfill({
            json: {
              directories: [
                dir("@@c\\Music", ["readme.txt"]),
                dir("@@c\\Music\\Albums", ["1.flac"])
              ],
              directoryCount: 2,
              lockedDirectories: [],
              lockedDirectoryCount: 0
            }
          })
      );
      await page.goto("./#/browse?user=carol");
      const shares = page.getByRole("treegrid", { name: "Shared folders" });
      const albums = shares.getByText("Albums", { exact: true });
      const readme = shares.getByText("readme.txt", { exact: true });
      await expect(readme).toBeVisible();
      expect(await apart(albums, readme), "browse").toBeLessThan(0.5);

      // the downloads, each user a group and their folders under it. A phone's row has no tick of
      // its own, so its icon starts under the tick of a header at its depth
      await page.goto("./#/downloads");
      const grid = page.getByRole("treegrid", { name: "Downloads" });
      const download = grid.getByTitle("@@a\\Music\\Album\\01 One.flac", { exact: true });
      await expect(download).toBeVisible();
      if (width < 768) {
        const tick = grid.getByRole("checkbox", { name: "Select every transfer in Bonus" });
        const icon = download.locator("xpath=preceding-sibling::*[1]");
        expect(await apart(tick, icon), "downloads").toBeLessThan(0.5);
      } else {
        const subfolder = grid.getByTitle("Bonus", { exact: true });
        expect(await apart(subfolder, download), "downloads").toBeLessThan(0.5);
      }
    });
  });
}
