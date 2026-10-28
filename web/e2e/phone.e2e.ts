import { expect, type Page, test } from "@playwright/test";

import { WIDEST } from "../src/lib/format";
import {
  fakeBrowse,
  fakeChat,
  fakeDownloads,
  fakeFiles,
  fakeFolder,
  fakeReports,
  fakeSearch,
  SEARCH_ID
} from "./fakes";
import { login } from "./login";

// a phone's width: no screen may be wider than it, so nothing runs off its right edge
test.use({ viewport: { width: 390, height: 844 } });

/**
 * Waits for `ready`, then checks the content area holds the screen: no sideways scroll, and
 * nothing cut off at its right edge by a box that hides what overflows it. A list that scrolls
 * sideways on its own is fine
 */
const holds = async (page: Page, what: string, ready: (page: Page) => Promise<void>) => {
  await ready(page);
  // the window itself never scrolls or zooms out: the content area is what scrolls
  const view = await page.evaluate(() => ({
    width: innerWidth,
    tall: document.documentElement.scrollHeight - document.documentElement.clientHeight,
    wide: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    scrolled: scrollY
  }));
  const width = page.viewportSize()!.width;
  expect(view, `${what} moves the window`).toEqual({ width, tall: 0, wide: 0, scrolled: 0 });
  const [scroll, client] = await page
    .locator("[data-content]")
    .evaluate((e) => [e.scrollWidth, e.clientWidth]);
  expect(scroll, `${what} is ${scroll - client}px wider than its screen`).toBeLessThanOrEqual(
    client
  );
  const cut = await page.locator("[data-content]").evaluate((area) => {
    const edge = area.getBoundingClientRect().right + 0.5;
    const scrolls = (e: Element) => ["auto", "scroll"].includes(getComputedStyle(e).overflowX);
    for (const e of area.querySelectorAll("main *")) {
      const box = e.getBoundingClientRect();
      if (!box.width || box.right <= edge || getComputedStyle(e).visibility === "hidden") continue;
      let inside = false;
      for (let a = e.parentElement; a && a !== area; a = a.parentElement) {
        if (scrolls(a)) inside = true;
      }
      if (!inside) return `${e.tagName.toLowerCase()} "${e.textContent?.trim().slice(0, 40)}"`;
    }
    return null;
  });
  expect(cut, `${what} cuts off ${cut}`).toBeNull();
  // a figure is never cut or painted over its neighbour, and a name never shrinks to nothing
  const cells = await page.locator("[data-content]").evaluate((area) => {
    const shown = (e: Element) => e.getClientRects().length > 0;
    for (const e of area.querySelectorAll<HTMLElement>(".tabular-nums")) {
      const style = getComputedStyle(e);
      const kept = style.whiteSpace === "nowrap" || style.textOverflow === "ellipsis";
      if (shown(e) && kept && e.scrollWidth > e.clientWidth + 1) {
        return `the figure "${e.textContent?.trim()}" needs ${e.scrollWidth}px of ${e.clientWidth}px`;
      }
    }
    for (const e of area.querySelectorAll<HTMLElement>(
      "[role=row] .truncate, [role=treeitem] .truncate"
    )) {
      if (shown(e) && e.textContent?.trim() && e.getBoundingClientRect().width < 8) {
        return `the name "${e.textContent.trim()}" has no room`;
      }
    }
    return null;
  });
  expect(cells, `${what}: ${cells}`).toBeNull();
};

/** Opens `path` and checks it fits */
const fits = async (page: Page, path: string, ready: (page: Page) => Promise<void>) => {
  await page.goto(`./#${path}`);
  await holds(page, path, ready);
};

const shows = (text: string | RegExp) => async (page: Page) =>
  expect(page.locator("main").getByText(text).first()).toBeVisible();

test("every screen fits a phone's width", async ({ page }) => {
  await fakeReports(page);
  await fakeSearch(page);
  await fakeChat(page);
  await fakeBrowse(page);
  await fakeFolder(page);
  await fakeFiles(page);
  await fakeDownloads(page);
  await login(page, "/dashboard");

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
  await page.getByRole("button", { name: "Show whole folder" }).first().click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("04 A Long Track Name.flac")).toBeVisible();
  for (const button of await dialog.getByRole("button").all()) {
    const box = (await button.boundingBox())!;
    expect(box.x, "a dialog button starts off the screen").toBeGreaterThanOrEqual(0);
    expect(box.x + box.width, "a dialog button ends off the screen").toBeLessThanOrEqual(390);
  }
  await page.keyboard.press("Escape");
  await page.getByRole("radio", { name: "Dense" }).click();
  await holds(page, "the dense results", shows("02 Two.flac"));
  await page.getByRole("radio", { name: "Cards" }).click();
  await fits(page, "/downloads", shows("3.flac"));
  await fits(page, "/uploads", shows("No uploads yet"));
  await fits(page, "/chat", shows("indie"));
  await fits(page, "/chat/rooms/indie", shows("anyone have the new boards of canada rip?"));
  await fits(page, "/chat/alice", shows("yes, browse away"));
  await fits(page, "/users", shows("No users yet"));
  await fits(page, "/users?tab=blocked", shows("No blocked users"));
  await fits(page, "/users/alice", shows("Not seen before"));
  await fits(page, "/browse", shows("Browse a user's shares"));
  await page.getByLabel("Username").fill("carol");
  await page.getByLabel("Username").press("Enter");
  await page.getByRole("treeitem", { name: /Bootlegs/ }).click();
  await holds(page, "a browse with a folder open", shows("2.flac"));
  await fits(page, "/profile", shows("How others see you"));
  for (const [tab, text] of [
    ["info", "Process ID"],
    ["settings", "Listen port"],
    ["account", "API tokens"],
    ["shares", "No shares configured"],
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
});

test.describe("on a wide screen", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the dashboard and the dense results hold their figures", async ({ page }) => {
    await fakeReports(page);
    await fakeSearch(page);
    await login(page, "/dashboard");
    await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
    await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
    await page.getByRole("radio", { name: "Dense" }).click();
    await holds(page, "the dense results", shows("02 Two.flac"));
    await page.getByRole("radio", { name: "Cards" }).click();
  });

  test("the widest figures fit their width in ch", async ({ page }) => {
    // a tabular digit is wider than 1ch, the plain zero; the narrower point, space and
    // separators of each formatter's widest value make up for it
    await login(page, "/dashboard");
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
});

test.describe("at a large text size", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the figure columns grow with the text", async ({ page }) => {
    // a browser's "Large" font size: the root 20 px, so the app's 14 px text is 17.5 px; set
    // before every page, since a navigation may load a new one
    await page.addInitScript(() =>
      document.addEventListener("DOMContentLoaded", () => {
        document.documentElement.style.fontSize = "20px";
      })
    );
    await fakeReports(page);
    await fakeSearch(page);
    await fakeDownloads(page);
    await login(page, "/dashboard");
    await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
    await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
    await page.getByRole("radio", { name: "Dense" }).click();
    await holds(page, "the dense results at 20 px", shows("02 Two.flac"));
    await page.getByRole("radio", { name: "Cards" }).click();
    await fits(page, "/downloads", shows("3.flac"));
  });
});
