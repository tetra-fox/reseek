import { expect, type Locator, test } from "@playwright/test";

import {
  fakeDownloadList,
  fakeSearch,
  response,
  SEARCH_ID,
  transfer,
  userDownloads
} from "./support/fakes";
import { textSize, visit } from "./support/login";

const ALBUM = "@@a\\Music\\Artist\\Album";
/** forty tracks on each of an album's two discs, a top folder with two subfolders */
const TRACKS = ["CD1", "CD2"].flatMap((disc) =>
  Array.from(
    { length: 40 },
    (_, n) => `${ALBUM}\\${disc}\\${String(n + 1).padStart(2, "0")} Track.flac`
  )
);

/**
 * The pinned lines in `box` that show, top first, each by its key past the last NUL, which is a
 * user's key or a folder's path
 */
const shownPins = (box: Locator) =>
  box.locator("[data-pinned]").evaluateAll((pins) =>
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
      .map(({ pin }) =>
        pin.dataset.line
          ?.replace(/^folder:/, "")
          .split("\u0000")
          .pop()
      )
  );

/** The root size the tests run at, larger than the browser's default */
const root = 24;

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  test.describe(`at ${width} px, a ${root} px root`, () => {
    test.use({ viewport: { width, height } });
    const phone = width < 768;

    test("a deep folder's line stays in view under the user's", async ({ page }) => {
      await textSize(page, root);
      await fakeSearch(page);
      await page.route(`**/api/v0/searches/${SEARCH_ID}/responses**`, (route) =>
        route.fulfill({ json: [response("alice", 3_000_000, TRACKS)] })
      );
      await visit(page, `/searches/${SEARCH_ID}`);
      const results = page.getByRole("treegrid", { name: "Search results" });
      await expect(results.getByText("01 Track.flac").first()).toBeVisible();
      const row = await results
        .locator("[role=rowgroup] [role=row]")
        .first()
        .evaluate((r) => r.getBoundingClientRect().height);
      // the user, the album, CD1, then its tracks: the 30th is the 33rd line. The tree's top in
      // view to see its pinned lines
      await results.evaluate((g) => g.scrollIntoView({ block: "start" }));
      await results.evaluate((b, y) => b.scrollTo({ top: y }), 32 * row - 2 * row);
      await expect
        .poll(() => shownPins(results))
        .toEqual(
          phone ? ["user:0:alice", `${ALBUM}\\CD1`] : ["user:0:alice", ALBUM, `${ALBUM}\\CD1`]
        );
      // in the next disc its line takes the first disc's place
      await results.evaluate((b, y) => b.scrollTo({ top: y }), 73 * row - 2 * row);
      await expect
        .poll(() => shownPins(results))
        .toEqual(
          phone ? ["user:0:alice", `${ALBUM}\\CD2`] : ["user:0:alice", ALBUM, `${ALBUM}\\CD2`]
        );
    });

    test("a deep folder's header stays under its user's in the downloads", async ({ page }) => {
      await textSize(page, root);
      const files = TRACKS.map((t) => transfer(t));
      await fakeDownloadList(page, () => [userDownloads("alice", files)]);
      await visit(page, "/downloads");
      const grid = page.getByRole("treegrid", { name: "Downloads" });
      const titles = () => shownPins(grid);
      await expect(grid.getByText("01 Track.flac", { exact: true }).first()).toBeVisible();

      // the middle of the first disc, then of the second, the grid's top in view to see them
      await grid.evaluate((g) => g.scrollIntoView({ block: "start" }));
      const rowHeight = await grid
        .getByRole("row")
        .nth(3)
        .evaluate((r) => r.getBoundingClientRect().height);
      await grid.evaluate((g, y) => g.scrollTo({ top: y }), rowHeight * 25);
      await expect
        .poll(titles)
        .toEqual(phone ? ["user:alice", `${ALBUM}\\CD1`] : ["user:alice", ALBUM, `${ALBUM}\\CD1`]);
      await grid.evaluate((g, y) => g.scrollTo({ top: y }), rowHeight * 65);
      await expect
        .poll(titles)
        .toEqual(phone ? ["user:alice", `${ALBUM}\\CD2`] : ["user:alice", ALBUM, `${ALBUM}\\CD2`]);
    });
  });
}
