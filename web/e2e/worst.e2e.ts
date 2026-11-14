import { expect, type Page, test } from "@playwright/test";

import { SEARCH_ID } from "./support/fakes";
import { fits, holds, shows } from "./support/fits";
import {
  NAMES,
  ROOMS,
  worstBrowse,
  worstChat,
  worstDirectory,
  worstProfile,
  worstSearch,
  worstTransferLists
} from "./support/worst";

/**
 * How many times `part` is drawn whole in the content area: each place it is in the text, its
 * box inside every box above it that hides what overflows
 */
const drawnWhole = (page: Page, part: string) =>
  page.locator("[data-content]").evaluate((area, part) => {
    const walker = document.createTreeWalker(area, NodeFilter.SHOW_TEXT);
    let whole = 0;
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const at = node.textContent!.indexOf(part);
      if (at < 0) continue;
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + part.length);
      const box = range.getBoundingClientRect();
      let clipped = !box.width;
      for (let e = node.parentElement; e && !clipped; e = e.parentElement) {
        if (getComputedStyle(e).overflowX === "visible") continue;
        const edge = e.getBoundingClientRect();
        clipped = box.left < edge.left - 0.5 || box.right > edge.right + 0.5;
      }
      if (!clipped) whole++;
    }
    return whole;
  }, part);

/**
 * The figures of `texts` the content area draws over more than one line, each looked for within a
 * text of its own; one found in no text is named as missing
 */
const brokenFigures = (page: Page, texts: string[]) =>
  page.locator("[data-content]").evaluate((area, texts) => {
    const broken: string[] = [];
    for (const text of texts) {
      const walker = document.createTreeWalker(area, NodeFilter.SHOW_TEXT);
      let found = false;
      for (let node = walker.nextNode(); node && !found; node = walker.nextNode()) {
        const at = node.textContent!.indexOf(text);
        if (at < 0) continue;
        found = true;
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + text.length);
        const lines = new Set([...range.getClientRects()].map((r) => Math.round(r.top)));
        if (lines.size !== 1) broken.push(text);
      }
      if (!found) broken.push(`${text} (missing)`);
    }
    return broken;
  }, texts);

/** Checks the two long file names that differ only at their ends show those ends */
const endsShow = async (page: Page, what: string) => {
  for (const end of ["_part1.flac", "_part2.flac"]) {
    expect(await drawnWhole(page, end), `${what}: no "${end}" drawn whole`).toBeGreaterThan(0);
  }
};

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  test.describe(`at ${width} px`, () => {
    test.use({ viewport: { width, height } });

    test("the worst search holds its screen in the tree and the list", async ({ page }) => {
      await worstSearch(page);
      await fits(page, "/searches", shows("Ultimate_Collection_of_Progressive"));
      await fits(page, `/searches/${SEARCH_ID}`, shows("_part2.flac"));
      await page.getByRole("radio", { name: "List" }).click();
      await holds(page, "the results list", shows("_part2.flac"));
      if (width > 390) {
        // a queue past what the column writes in full keeps the column's width
        await expect(page.getByText("99,999+").first()).toBeVisible();
        await endsShow(page, "the results list");
      }
    });

    test("the worst browse holds its screen and shows no figure a peer can't mean", async ({
      page
    }) => {
      await worstBrowse(page);
      await fits(page, `/users/${encodeURIComponent(NAMES.long)}/shares`, shows("Various_Artists"));
      await page.locator("[data-line$='Unmixed_Versions_Included)']").first().click();
      await holds(page, "a browse with the long folder open", shows("0002 Track.flac"));
      // a length or bitrate of -1, read unsigned, is unknown, as in a search
      await expect(page.getByText(/1193046|4294967295|4294967\.295/)).toHaveCount(0);
    });

    for (const path of ["/downloads", "/uploads"]) {
      for (const groupBy of ["user", "flat"]) {
        test(`the worst ${path.slice(1)}, ${groupBy}, hold their screen and show the ends of names`, async ({
          page
        }) => {
          await worstTransferLists(page);
          const direction = path === "/downloads" ? "download" : "upload";
          await page.addInitScript(
            ([key, groupBy]) => localStorage.setItem(key, JSON.stringify({ groupBy })),
            [`transfers.view.${direction}`, groupBy] as const
          );
          await fits(page, path, shows("_part2.flac"));
          await endsShow(page, `${path}, ${groupBy}`);
        });
      }
    }

    test("the worst user's profile holds its screen", async ({ page }) => {
      await worstProfile(page);
      await fits(page, `/users/${encodeURIComponent(NAMES.long)}`, shows("Upload slots"));
      await expect(page.getByText(/Upload slots\s*4,294,967,295/)).toBeVisible();
      // the header's lines break between figures, never inside one
      const figures = [
        "9,999,999 files",
        "2,400,000 folders",
        "4 GB/s",
        "4,294,967,295 uploads",
        "9,999,999 downloads (0 B)",
        "2,400,000 uploads (200 TB)",
        "1,000,000 messages"
      ];
      expect(await brokenFigures(page, figures)).toEqual([]);
      // the description of a megabyte is folded, so what is under it is a scroll away
      const more = page.getByRole("button", { name: "Show more" });
      await expect(more).toBeVisible();
      const notes = await page.getByRole("heading", { name: "Notes" }).boundingBox();
      expect(notes!.y, "the notes are under the whole description").toBeLessThan(4000);
      await more.click();
      await expect(more).toHaveCount(0);
    });

    test("the worst users list holds its screen", async ({ page }) => {
      await worstDirectory(page);
      await fits(page, "/users", shows(NAMES.cjk));
    });

    test("the worst room and conversation hold their screens", async ({ page }) => {
      await worstChat(page);
      await fits(page, "/chat", shows(ROOMS.cjk));
      await fits(page, `/chat/rooms/${encodeURIComponent(ROOMS.rtl)}`, shows("مرحبا بالجميع"));
      await fits(page, `/chat/${encodeURIComponent(NAMES.long)}`, shows("Show more"));
    });
  });
}
