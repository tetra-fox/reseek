import { expect, test } from "@playwright/test";

import { fakeBrowse, fakeChat, fakeSearch, manyDownloads, SEARCH_ID } from "./fakes";
import { login } from "./login";

test("shows search results as cards and as dense rows walked by key", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/dashboard");
  await page.goto(`./#/searches/${SEARCH_ID}`);
  const crumbs = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(crumbs).toContainText("Search");
  await expect(crumbs).toContainText("album");

  // cards: one per user, folders as a tree under the chain they share
  const alice = page.locator('section[data-key="0:alice"]');
  await expect(alice.getByRole("button", { name: "Close @@a\\Music" })).toBeVisible();
  await expect(alice.getByText("01 One.flac")).toBeVisible();

  // a filter token keeps only what it matches, and the clear button empties the box
  const filter = page.getByLabel("Filter", { exact: true });
  await filter.fill("type:mp3");
  await expect(alice.getByText("03 Three.mp3")).toBeVisible();
  // a result's row acts on a click and shows it; the count above them is plain text
  await expect(alice.locator("[role=row][data-line]").last()).toHaveCSS("cursor", "pointer");
  await expect(page.getByRole("status").filter({ hasText: "files from" })).toHaveCSS(
    "cursor",
    "default"
  );
  await expect(page.locator('section[data-key="0:bob"]')).toHaveCount(0);
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(filter).toHaveValue("");
  await expect(page.locator('section[data-key="0:bob"]')).toBeVisible();

  // the dense view: one row per file, walked with the keys
  await page.getByRole("radio", { name: "Dense" }).click();
  const grid = page.getByRole("grid", { name: "Search results" });
  await expect(grid.locator("[role=row][data-line]")).toHaveCount(5);
  await expect(grid.locator("[role=row][data-line]").first()).toHaveCSS("cursor", "pointer");
  await grid.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press(" ");
  const active = await grid.getAttribute("aria-activedescendant");
  await expect(page.locator(`#${active}`)).toContainText("02 Two.flac");
  await expect(grid.locator('[aria-selected="true"]')).toHaveCount(1);
  // sorting by file name keeps the selection on its file
  await grid.getByRole("button", { name: "File", exact: true }).click();
  await expect(grid.locator('[aria-selected="true"]')).toContainText("02 Two.flac");

  // the choice of view is remembered
  await page.reload();
  await expect(page.getByRole("radio", { name: "Dense" })).toBeChecked();
});

test("cross-fades between screens", async ({ page }) => {
  await page.addInitScript(() => {
    const start = document.startViewTransition?.bind(document);
    if (!start) return;
    Object.assign(window, { fades: 0 });
    document.startViewTransition = ((update: () => Promise<void>) => {
      (window as unknown as { fades: number }).fades += 1;
      return start(update);
    }) as typeof document.startViewTransition;
  });
  await login(page, "/dashboard");
  const fades = () => page.evaluate(() => (window as unknown as { fades: number }).fades);
  const before = await fades();
  await page.getByRole("link", { name: "Downloads" }).click();
  await expect(page).toHaveURL(/#\/downloads$/);
  await expect.poll(fades).toBeGreaterThan(before);
});

test("browses a user, finds a folder by typing and shows where it is", async ({ page }) => {
  await fakeBrowse(page);
  await login(page, "/browse");
  await page.getByLabel("Username").fill("carol");
  await page.getByLabel("Username").press("Enter");
  const tree = page.getByRole("tree", { name: "Shared folders" });
  await expect(tree.getByText("Bootlegs")).toBeVisible();
  // a folder is a pointer target, not text to select
  await expect(tree.getByRole("treeitem", { name: /Bootlegs/ })).toHaveCSS("cursor", "pointer");
  await tree.focus();
  await page.keyboard.type("bo");
  await expect(tree.getByRole("treeitem", { name: /Bootlegs/ })).toHaveAttribute(
    "aria-selected",
    "true"
  );
  const crumbs = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(crumbs).toContainText("carol");
  await expect(crumbs).toContainText("Bootlegs");
  await expect(page.getByRole("button", { name: /Download this folder/ })).toBeVisible();
  // a step back up opens that folder
  await crumbs.getByRole("button", { name: "Music" }).click();
  const chosen = tree.locator('[role=treeitem][aria-selected="true"]');
  await expect(chosen).toContainText("Music");
  await expect(chosen).not.toContainText("Bootlegs");
  // carol is remembered on the start screen
  await page.getByRole("button", { name: "Clear" }).first().click();
  await expect(
    page.getByRole("region", { name: "Recently browsed" }).getByText("carol")
  ).toBeVisible();
});

test("goes to a screen from the command palette", async ({ page }) => {
  await login(page, "/dashboard");
  // the palette is part of the shell, there once the app is up
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+k");
  const box = page.getByRole("combobox", { name: /Go to a screen/ });
  await expect(box).toBeFocused();
  await box.fill("downl");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/downloads$/);
  await expect(box).toBeHidden();
});

test("keeps a compact row's controls between its dividers", async ({ page }) => {
  await manyDownloads(page, 30);
  await login(page, "/downloads");
  await page.getByRole("radio", { name: "Compact" }).click();
  const grid = page.getByRole("grid", { name: "Downloads" });
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

test("shows a list's focus ring for the keys, not after a click", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/chat");
  const list = page.getByRole("grid", { name: "Rooms and conversations" });
  const rooms = list.locator('[data-line="h:rooms"]');
  await rooms.getByRole("button").click();
  await page.mouse.move(0, 0);
  await expect(list.locator("[data-line]").first()).toBeVisible();
  await expect(rooms).toHaveCSS("box-shadow", "none");
  await expect(list.locator("[data-focused]")).toHaveCount(0);
  // the keys bring it back, on the row they move to
  await rooms.getByRole("button").click();
  await page.keyboard.press("ArrowDown");
  await expect(list.locator("[data-focused]")).toHaveCount(1);
  await expect(list.locator("[data-focused]")).not.toHaveCSS("box-shadow", "none");
});

test("fades between screens without morphing the content area", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/dashboard");
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
