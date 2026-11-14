import { expect, test } from "@playwright/test";

import { fakeBrowse, fakeChat, fakeSearch, manyDownloads, SEARCH_ID } from "./support/fakes";
import { open, visit } from "./support/login";

test("shows search results as a tree and as a list walked by key", async ({ page }) => {
  await fakeSearch(page);
  await visit(page, "/dashboard");
  await open(page, `/searches/${SEARCH_ID}`);
  const crumbs = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(crumbs).toContainText("Search");
  await expect(crumbs).toContainText("album");

  // one tree: a line per user, their folders under the chain they share
  const tree = page.getByRole("treegrid", { name: "Search results" });
  const user = (name: string) => tree.locator(`[data-line="folder:user:0:${name}"]`);
  await expect(user("alice")).toBeVisible();
  await expect(tree.getByRole("button", { name: "Close @@a\\Music" })).toBeVisible();
  await expect(tree.getByText("01 One.flac")).toBeVisible();

  // a filter token keeps only what it matches, and the clear button empties the box
  const filter = page.getByLabel("Filter", { exact: true });
  await filter.fill("type:mp3");
  await expect(tree.getByText("03 Three.mp3")).toBeVisible();
  await expect(user("bob")).toHaveCount(0);
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(filter).toHaveValue("");
  await expect(user("bob")).toBeVisible();

  // the list: one row per file, walked with the keys
  await page.getByRole("radio", { name: "List" }).click();
  const grid = page.getByRole("treegrid", { name: "Search results" });
  await expect(grid.locator("[role=row][data-line]")).toHaveCount(5);
  await grid.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Shift+Space");
  const active = await grid.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toContainText("02 Two.flac");
  await expect(grid.locator('[aria-selected="true"]')).toHaveCount(1);
  // sorting by file name keeps the selection on its file
  await grid.getByRole("button", { name: "File", exact: true }).click();
  await expect(grid.locator('[aria-selected="true"]')).toContainText("02 Two.flac");

  // the choice of view is remembered
  await page.reload();
  await expect(page.getByRole("radio", { name: "List" })).toBeChecked();
});

test("browses a user, finds a folder by typing and shows where it is", async ({ page }) => {
  await fakeBrowse(page);
  await visit(page, "/users/carol/shares");
  const tree = page.getByRole("treegrid", { name: "Shared folders" });
  const bootlegs = tree.locator('[data-line$="Bootlegs"]');
  await expect(bootlegs).toBeVisible();
  await tree.focus();
  await page.keyboard.type("bo");
  await expect(bootlegs).toHaveAttribute("data-focused", "true");
  const crumbs = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(crumbs).toContainText("carol");
  await expect(crumbs).toContainText("Bootlegs");
  // a step back up goes to that folder's line
  await crumbs.getByRole("button", { name: "Music" }).click();
  const active = await tree.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toHaveAttribute("data-line", "folder:@@c\\Music");
  // carol is remembered under the search box
  await open(page, "/searches");
  const shares = page.getByRole("list", { name: "Recently browsed" }).getByRole("link");
  await expect(shares).toHaveText("carol");
  await shares.click();
  await expect(page).toHaveURL(/#\/users\/carol\/shares$/);
});

test("keeps a compact row's controls between its dividers", async ({ page }) => {
  await manyDownloads(page, 30);
  await visit(page, "/downloads");
  await page.getByRole("button", { name: "Density: Comfortable" }).click();
  await expect(page.getByRole("button", { name: "Density: Compact" })).toBeVisible();
  const grid = page.getByRole("treegrid", { name: "Downloads" });
  const row = grid.locator("[role=row]").filter({ hasText: "00002 Track.flac" });
  const header = grid.locator("[role=row]").filter({ hasText: "Album 0" }).first();
  for (const line of [row, header]) {
    await line.getByRole("button").last().hover();
    // every control, and so its hover tint, ends above the line's divider
    const crossing = await line.evaluate((e) => {
      const box = e.getBoundingClientRect();
      const divider = parseFloat(getComputedStyle(e).borderBottomWidth);
      return [...e.querySelectorAll("button")]
        .map((b) => b.getBoundingClientRect())
        .filter((b) => b.height && (b.top < box.top || b.bottom > box.bottom - divider)).length;
    });
    expect(crossing).toBe(0);
    await expect(line).toHaveCSS("border-bottom-width", "1px");
  }
});

test("a row's hidden menu takes no width, and comes up without moving the row", async ({
  page
}) => {
  await fakeChat(page);
  await visit(page, "/chat");
  const row = page.locator('[data-line="room:indie"]');
  const text = row.getByRole("gridcell");
  const menu = row.getByRole("button", { name: "Menu for indie" });
  await expect(row).toBeVisible();
  await page.mouse.move(0, 0);
  await expect(menu).toHaveCSS("opacity", "0");
  // the text runs to the row's padding with nothing kept for the control
  const inner = await row.evaluate((e) => {
    const box = e.getBoundingClientRect();
    return box.right - parseFloat(getComputedStyle(e).paddingRight);
  });
  const rest = (await text.boundingBox())!;
  expect(Math.abs(rest.x + rest.width - inner)).toBeLessThanOrEqual(1);
  await row.hover();
  await expect(menu).toHaveCSS("opacity", "1");
  expect(await text.boundingBox()).toEqual(rest);
  // the keys reach it with the pointer away
  await page.mouse.move(0, 0);
  await expect(menu).toHaveCSS("opacity", "0");
  await page.getByRole("grid", { name: "Rooms and conversations" }).focus();
  // past the section's own toggle, the first stop in the list
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(menu).toBeFocused();
  await expect(menu).toHaveCSS("opacity", "1");
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("a row's menu is always there for a finger", async ({ page }) => {
    await fakeChat(page);
    await visit(page, "/chat");
    const menu = page.getByRole("button", { name: "Menu for indie" });
    await expect(menu).toHaveCSS("opacity", "1");
    await expect(menu).toHaveCSS("position", "static");
    await menu.tap();
    await expect(page.getByRole("menuitem", { name: "Pin", exact: true })).toBeVisible();
  });
});

test("fades between screens without morphing the content area", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/dashboard");
  // the size each named part of a transition starts and ends at, as the transitions run
  await page.evaluate(() => {
    const seen: string[] = [];
    Object.assign(window, { seen });
    const watch = () => {
      for (const a of document.getAnimations()) {
        const effect = a.effect as KeyframeEffect;
        if (!effect.pseudoElement?.startsWith("::view-transition-group")) continue;
        const frames = effect.getKeyframes();
        const size = (k: Keyframe) => `${k.width} x ${k.height}`;
        seen.push(`${effect.pseudoElement}: ${size(frames[0])} to ${size(frames.at(-1)!)}`);
      }
      requestAnimationFrame(watch);
    };
    watch();
  });
  const seen = () => page.evaluate(() => (window as unknown as { seen: string[] }).seen);
  // from a padded screen to the full-height chat, and back
  await page.getByRole("link", { name: "Chat" }).click();
  await expect(page).toHaveURL(/#\/chat$/);
  await page.getByRole("link", { name: "Dashboard" }).click();
  await expect(page).toHaveURL(/#\/dashboard$/);
  await expect
    .poll(async () => (await seen()).some((s) => s.startsWith("::view-transition-group(page)")))
    .toBe(true);
  for (const s of await seen()) {
    const [from, to] = s.split(": ")[1].split(" to ");
    expect(to, s).toBe(from);
  }
});

test("reads a shortcut's keys as words where it draws them as signs", async ({ page }) => {
  await visit(page, "/dashboard");
  const activity = page.getByRole("button", { name: "Activity", exact: true });
  await activity.hover();
  await expect(activity).toHaveAccessibleDescription(
    "Activity log: browses, searches and downloads by other users Control+J"
  );
  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByRole("option", { name: "Activity log Control+J" })).toBeVisible();
  await expect(page.getByRole("option", { name: "Keyboard shortcuts ?" })).toBeVisible();
});

for (const [name, viewport] of [
  ["a laptop", { width: 1280, height: 720 }],
  ["a phone", { width: 390, height: 844 }]
] as const) {
  test(`fits the filter syntax in the window on ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await fakeSearch(page);
    await visit(page, "/dashboard");
    await open(page, `/searches/${SEARCH_ID}`);
    await page.getByRole("button", { name: "Filter syntax" }).click();
    const help = page.getByText("Every word must appear").locator("xpath=..");
    await expect(help).toBeVisible();
    const box = (await help.boundingBox())!;
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
    // what does not fit scrolls inside it, down to the last token
    const last = help.locator("dd").last();
    await last.scrollIntoViewIfNeeded();
    await expect(last).toBeInViewport();
  });
}
