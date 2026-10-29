import { expect, type Locator, test } from "@playwright/test";

import { fakeSearch, file, SEARCH_ID, transfer } from "./fakes";
import { login, textSize } from "./login";

const ALBUM = "@@a\\Music\\Artist\\Album";
/** forty tracks on each of an album's two discs, a top folder with two subfolders */
const TRACKS = ["CD1", "CD2"].flatMap((disc) =>
  Array.from(
    { length: 40 },
    (_, n) => `${ALBUM}\\${disc}\\${String(n + 1).padStart(2, "0")} Track.flac`
  )
);

/** The element's box on the page */
const box = async (locator: Locator) => {
  const found = await locator.boundingBox();
  if (!found) throw new Error("not on the page");
  return found;
};

/** Whether the element is what shows at its own middle, nothing drawn over it */
const onTop = (locator: Locator) =>
  locator.evaluate((element) => {
    const r = element.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return !!hit && element.contains(hit);
  });

/** Scrolls the page until `locator` is in the middle of the view */
const centre = (locator: Locator) =>
  locator.evaluate((element) => element.scrollIntoView({ block: "center" }));

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  for (const root of [16, 24]) {
    test.describe(`at ${width} px, a ${root} px root`, () => {
      test.use({ viewport: { width, height } });
      const phone = width < 768;

      test("a deep folder's line stays in view under the user's", async ({ page }) => {
        await textSize(page, root);
        await fakeSearch(page);
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
                fileCount: TRACKS.length,
                files: TRACKS.map((t) => file(t)),
                lockedFileCount: 0,
                lockedFiles: []
              }
            ]
          })
        );
        await login(page, `/searches/${SEARCH_ID}`);
        const card = page.locator('section[data-key="0:alice"]');
        // the header, of which a phone keeps only the user's line in view
        const user = phone ? card.locator("header > div").first() : card.locator("header");
        const row = (title: string) =>
          card.getByTitle(title, { exact: true }).locator("xpath=ancestor::*[@role='row'][1]");
        const album = row(ALBUM);
        const cd1 = row(`${ALBUM}\\CD1`);
        const cd2 = row(`${ALBUM}\\CD2`);

        await centre(card.getByTitle(`${ALBUM}\\CD1\\30 Track.flac`, { exact: true }));
        await expect.poll(() => onTop(cd1)).toBe(true);
        expect(await onTop(user)).toBe(true);
        const under = (await box(user)).y + (await box(user)).height;
        if (phone) {
          // a phone keeps the user and the innermost folder, which covers the album's line
          expect((await box(cd1)).y).toBeCloseTo(under, 0);
          expect(await onTop(album)).toBe(false);
        } else {
          expect((await box(album)).y).toBeCloseTo(under, 0);
          expect(await onTop(album)).toBe(true);
          const albumBox = await box(album);
          expect((await box(cd1)).y).toBeCloseTo(albumBox.y + albumBox.height, 0);
        }

        // in the next disc its line takes the first disc's place
        await centre(card.getByTitle(`${ALBUM}\\CD2\\30 Track.flac`, { exact: true }));
        await expect.poll(() => onTop(cd2)).toBe(true);
        expect(await onTop(cd1)).toBe(false);
        expect(await onTop(user)).toBe(true);
        if (!phone) expect(await onTop(album)).toBe(true);
      });

      test("a deep folder's header stays under its user's in the downloads", async ({ page }) => {
        await textSize(page, root);
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
                      fileCount: TRACKS.length,
                      files: TRACKS.map((t) => transfer(t))
                    }
                  ]
                }
              ]
            })
        );
        await login(page, "/downloads");
        const grid = page.getByRole("grid", { name: "Downloads" });
        const pinned = grid.locator("[data-pinned]");
        const titles = () =>
          pinned.evaluateAll((pins) =>
            pins
              .map((pin) => ({ pin, rect: pin.getBoundingClientRect() }))
              .filter(({ pin, rect }) => {
                const hit = document.elementFromPoint(
                  rect.left + rect.width / 2,
                  rect.top + rect.height / 2
                );
                return !!hit && pin.contains(hit);
              })
              .sort((a, b) => a.rect.top - b.rect.top)
              .map(({ pin }) => pin.dataset.line?.split("\u0000").pop())
          );
        await expect(
          grid.getByTitle(`${ALBUM}\\CD1\\01 Track.flac`, { exact: true })
        ).toBeVisible();

        // the middle of the first disc, then of the second, the grid's top in view to see them
        await grid.evaluate((g) => g.scrollIntoView({ block: "start" }));
        const rowHeight = await grid
          .getByRole("row")
          .nth(3)
          .evaluate((r) => r.getBoundingClientRect().height);
        await grid.evaluate((g, y) => g.scrollTo({ top: y }), rowHeight * 25);
        await expect
          .poll(titles)
          .toEqual(
            phone ? ["user:alice", `${ALBUM}\\CD1`] : ["user:alice", ALBUM, `${ALBUM}\\CD1`]
          );
        await grid.evaluate((g, y) => g.scrollTo({ top: y }), rowHeight * 65);
        await expect
          .poll(titles)
          .toEqual(
            phone ? ["user:alice", `${ALBUM}\\CD2`] : ["user:alice", ALBUM, `${ALBUM}\\CD2`]
          );
      });
    });
  }
}
