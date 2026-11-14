import { expect, type Page, test } from "@playwright/test";

import { WIDEST } from "../src/lib/format";
import {
  fakeAnalysis,
  fakeBrowse,
  fakeChat,
  fakeDownloads,
  fakeFiles,
  fakeFolder,
  fakeReports,
  fakeSearch,
  SEARCH_ID
} from "./support/fakes";
import { fits, holds, layerHolds, shows } from "./support/fits";
import { open, refuseCopy, textSize, visit } from "./support/login";

// a phone's width: no screen may be wider than it, so nothing runs off its right edge
test.use({ viewport: { width: 390, height: 844 } });

/**
 * From a finished download's row, checks the inspector holds the file and its analysis, then the
 * Analysis page with the analyzer's reading open, and comes back
 */
const analysisHolds = async (page: Page) => {
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Details" }).click();
  const inspector = page.locator("[data-inspector]");
  const technical = inspector.getByRole("button", { name: "Technical" });
  await expect(inspector.getByText("Likely a transcode").first()).toBeVisible();
  if ((await technical.getAttribute("aria-expanded")) === "false") await technical.click();
  await layerHolds(inspector, "inspector");
  await inspector.getByRole("button", { name: "Open in Analysis" }).click();
  const slot = page.getByRole("region", { name: "Slot A" });
  await slot.getByText("Analyzer reading", { exact: true }).click();
  await slot.getByRole("radio", { name: "Mel" }).click();
  await expect(slot.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("button", { name: "Guide" }).click();
  await expect(page.getByText(/The analysis is a guide, not proof/)).toBeVisible();
  await page.keyboard.press("Escape");
  await layerHolds(slot, "Analysis page");
  await page.goBack();
  await expect(slot).toBeHidden();
};

/** Opens every screen in turn, with the data that fills each, and checks each fits */
const everyScreen = async (page: Page) => {
  await fakeReports(page);
  await fakeSearch(page);
  await fakeChat(page);
  await fakeBrowse(page);
  await fakeFolder(page);
  await fakeFiles(page);
  await fakeDownloads(page);
  await fakeAnalysis(page);
  // the list of searches as a fresh daemon has it, whatever other tests searched for
  await page.route("**/api/v0/searches", (route) =>
    route.request().method() === "GET" ? route.fulfill({ json: [] }) : route.fallback()
  );
  await visit(page, "/dashboard");

  await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
  for (const tab of ["Folders", "Errors"]) {
    await page.getByRole("button", { name: tab, exact: true }).click();
    await holds(
      page,
      `the dashboard's ${tab} tab`,
      shows(tab === "Folders" ? "Most uploaded folders" : "Recent errors")
    );
  }
  await fits(page, "/searches", shows("No searches yet"));
  await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
  // the whole folder's dialog keeps its buttons on the screen
  await page.getByRole("button", { name: "View all files" }).first().click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("04 A Long Track Name.flac")).toBeVisible();
  for (const button of await dialog.getByRole("button").all()) {
    const box = (await button.boundingBox())!;
    expect(box.x, "a dialog button starts off the screen").toBeGreaterThanOrEqual(0);
    expect(box.x + box.width, "a dialog button ends off the screen").toBeLessThanOrEqual(390);
  }
  await page.keyboard.press("Escape");
  await page.getByRole("radio", { name: "List" }).click();
  await holds(page, "the results list", shows("02 Two.flac"));
  await page.getByRole("radio", { name: "Tree" }).click();
  await fits(page, "/downloads", shows("3.flac"));
  // a phone shows no Columns menu, and sorts by any column from the Sort menu
  await expect(page.getByRole("button", { name: "Columns" })).toHaveCount(0);
  await page.getByRole("button", { name: /^Sort:/ }).click();
  await page.getByRole("menuitemradio", { name: "Speed" }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Sort: Speed" })).toBeVisible();
  await analysisHolds(page);
  await fits(page, "/uploads", shows("No uploads yet"));
  await fits(page, "/chat", shows("indie"));
  await fits(page, "/chat/rooms/indie", shows("anyone have the new boards of canada rip?"));
  await fits(page, "/chat/alice", shows("yes, browse away"));
  await fits(page, "/chat/server", shows("is now a member of room"));
  await fits(page, "/chat/search?q=indie", (p) =>
    expect(p.getByRole("grid", { name: "Search results" })).toContainText("indie")
  );
  await fits(page, "/users", shows("No users yet"));
  await fits(page, "/users?tab=blocked", shows("No blocked users"));
  await fits(page, "/users/alice", shows("Not seen before"));
  await fits(page, "/users/carol/shares", shows("Bootlegs"));
  await page.locator('[data-line$="Bootlegs"]').click();
  await holds(page, "a browse with a folder open", shows("2.flac"));
  await fits(page, "/profile", shows("What others see"));
  for (const [tab, text] of [
    ["info", "Process ID"],
    ["settings", "Listen port"],
    ["account", "API tokens"],
    ["shares", "No shares"],
    ["files", "Kid A (Collector's Edition)"],
    ["data", "Finished transfers"],
    ["events", /types/],
    ["logs", "Live"]
  ] as const) {
    await fits(page, `/system/${tab}`, shows(text));
  }

  // past the end of a long screen, the wheel moves nothing else
  await fits(page, "/system/settings", shows("Listen port"));
  await page.locator("[data-content]").evaluate((e) => e.scrollTo({ top: e.scrollHeight }));
  for (let i = 0; i < 6; i++) await page.mouse.wheel(0, 400);
  await holds(page, "settings scrolled past the end", shows("Listen port"));

  // the activity log's button, its only opener on a phone, is on the screen
  const button = (await page.getByRole("button", { name: "Activity" }).boundingBox())!;
  expect(button.x + button.width).toBeLessThanOrEqual(390);
};

test.describe("on a wide screen", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the widest figures fit their width in ch", async ({ page }) => {
    // a tabular digit is wider than 1ch, the plain zero; the narrower point, space and
    // separators of each formatter's widest value make up for it
    await visit(page, "/dashboard");
    const over = await page.evaluate((widest) => {
      const span = (text: string, width?: string) => {
        const el = document.createElement("span");
        el.className = "text-sm whitespace-pre tabular-nums";
        el.textContent = text;
        if (width) Object.assign(el.style, { display: "inline-block", width });
        document.body.append(el);
        const w = el.getBoundingClientRect().width;
        el.remove();
        return w;
      };
      return widest
        .map((text) => ({ text, need: span(text), room: span("", `${text.length}ch`) }))
        .filter((f) => f.need > f.room);
    }, Object.values(WIDEST));
    expect(over).toEqual([]);
  });

  // the breakpoints are in rem, so at this width the root size picks the layout: 16 px lands on
  // `xl`, 20 px on `lg` and 24 px on `md`
  for (const root of [16, 20, 24]) {
    test(`the figure columns grow with the text at ${root} px`, async ({ page }) => {
      await textSize(page, root);
      await fakeReports(page);
      await fakeSearch(page);
      await fakeDownloads(page);
      await fakeAnalysis(page);
      await visit(page, "/dashboard");
      await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
      await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
      await page.getByRole("radio", { name: "List" }).click();
      await holds(page, `the results list at ${root} px`, shows("02 Two.flac"));
      await page.getByRole("radio", { name: "Tree" }).click();
      await fits(page, "/downloads", shows("3.flac"));
      await fits(page, "/system/info", shows("Process ID"));
    });
  }
});

// a phone is narrower than the smallest breakpoint at every root size, so there a layout only
// overflows more as the text grows and the largest size covers those between it and the default
test.describe("at a 24 px text size", () => {
  test("every screen fits a phone", async ({ page }) => {
    test.slow(); // it opens every screen in turn
    await textSize(page, 24);
    await everyScreen(page);
    // the activity log's sheet stands on the status bar, however many lines that has
    const bar = page.locator("footer:not(main footer)");
    const toggle = bar.getByRole("button", { name: "Activity" });
    await toggle.click();
    // polled, as the sheet slides up as it opens
    const gap = async () => {
      const sheet = (await page
        .getByRole("region", { name: "Activity on the network" })
        .boundingBox())!;
      return Math.round(sheet.y + sheet.height - (await bar.boundingBox())!.y);
    };
    await expect.poll(gap, { message: "the sheet's foot is off the bar" }).toBe(0);
  });
});

test("a toast's countdown runs along its foot and holds under the pointer", async ({ page }) => {
  await refuseCopy(page);
  await fakeChat(page);
  await visit(page, "/chat");
  await open(page, "/chat/rooms/indie");
  const log = page.getByRole("log");
  await log.getByText("thanks bob, grabbing it now").hover();
  await page.getByRole("button", { name: "Copy text" }).last().click();
  const toast = page.getByRole("alert").filter({ hasText: "Could not copy" });
  await expect(toast).toBeVisible();
  const bar = toast.locator(".countdown");
  // measured once it has flown in, as the two boxes are read one after the other
  await toast.evaluate((e) => Promise.all(e.getAnimations().map((a) => a.finished)));
  const [box, line] = [(await toast.boundingBox())!, (await bar.boundingBox())!];
  expect(box.x + box.width, "the toast fits the phone").toBeLessThanOrEqual(390);
  expect(line.height).toBeLessThanOrEqual(3);
  expect(line.x).toBeGreaterThanOrEqual(box.x);
  expect(line.x + line.width).toBeLessThanOrEqual(box.x + box.width);
  expect(Math.abs(line.y + line.height - (box.y + box.height))).toBeLessThanOrEqual(1);
  const countdown = () => bar.evaluate((e) => e.getAnimations()[0].playState);
  // the bar holds while the pointer is on the toast
  await toast.hover();
  await expect.poll(countdown).toBe("paused");
  await page.mouse.move(0, 0);
  await expect.poll(countdown).toBe("running");
});
