import { expect, type Page, test } from "@playwright/test";

import { fakeBrowse, fakeConnected, fakeDownloads, fakeSearch, SEARCH_ID } from "./support/fakes";
import { login } from "./support/login";

/** The pane's width before it is resized, in pixels */
const WIDTH = 384;

const pane = (page: Page) => page.getByRole("complementary", { name: "Details" });
const fileName = (page: Page) => pane(page).getByRole("heading", { level: 2 });

/** The Downloads screen with every transfer on a line of its own, focus in the table */
const flatDownloads = async (page: Page) => {
  await fakeDownloads(page);
  await login(page, "/downloads");
  await page.getByRole("radio", { name: "Flat" }).click();
  const grid = page.getByRole("treegrid", { name: "Downloads" });
  await expect(grid.getByText("3.flac")).toBeVisible();
  await grid.focus();
};

test.describe("on a wide screen", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("docks beside the list, which narrows, and keeps its width", async ({ page }) => {
    await flatDownloads(page);
    const content = page.locator("[data-content]");
    const before = (await content.boundingBox())!;

    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("i");
    await expect(pane(page)).toBeVisible();
    // polled, as the pane slides in by its width
    await expect.poll(async () => (await pane(page).boundingBox())!.width).toBe(WIDTH);
    const docked = (await pane(page).boundingBox())!;
    const narrowed = (await content.boundingBox())!;
    expect(narrowed.width).toBe(before.width - WIDTH);
    expect(docked.x, "the pane covers the list").toBeGreaterThanOrEqual(
      narrowed.x + narrowed.width
    );
    expect(docked.x + docked.width).toBe(before.x + before.width);

    // wider with the keys, and the same after a reload
    const handle = page.getByRole("separator", { name: "Width of the details pane" });
    await handle.focus();
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await expect.poll(async () => (await pane(page).boundingBox())!.width).toBe(WIDTH + 40);
    await page.reload();
    await expect(pane(page)).toBeVisible();
    await expect.poll(async () => (await pane(page).boundingBox())!.width).toBe(WIDTH + 40);
  });

  test("follows the arrow keys, and Space or i opens and closes it", async ({ page }) => {
    await flatDownloads(page);
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("Space");
    await expect(pane(page)).toBeVisible();
    const first = (await fileName(page).textContent())!;

    await page.keyboard.press("ArrowDown");
    await expect(fileName(page)).not.toHaveText(first);
    await page.keyboard.press("ArrowUp");
    await expect(fileName(page)).toHaveText(first);

    // the row it shows stays marked, with nothing ticked by Space
    const grid = page.getByRole("treegrid", { name: "Downloads" });
    await expect(grid.locator("[data-inspected]")).toContainText(first);
    await expect(grid.locator('[aria-selected="true"]')).toHaveCount(0);

    await page.keyboard.press("i");
    await expect(pane(page)).toBeHidden();
    await page.keyboard.press("Space");
    await expect(pane(page)).toBeVisible();
    await page.keyboard.press("Shift+Space");
    await expect(grid.locator('[aria-selected="true"]')).toHaveCount(1);
    await expect(pane(page)).toBeVisible();
  });

  test("an action leaves the keys in the list", async ({ page }) => {
    await flatDownloads(page);
    await page.getByRole("button", { name: /^2\.flac/ }).click();
    await expect(fileName(page)).toHaveText("2.flac");
    const open = page.waitForEvent("popup");
    await pane(page).getByRole("button", { name: "Open", exact: true }).click();
    await (await open).close();
    await page.keyboard.press("ArrowDown");
    await expect(fileName(page)).not.toHaveText("2.flac");
  });

  test("folds its sections, leaves out empty fields and remembers", async ({ page }) => {
    await fakeDownloads(page);
    await login(page, "/downloads");
    await page.getByRole("button", { name: "2.flac", exact: true }).click();

    // always in view: what the file is and what to do with it
    await expect(fileName(page)).toHaveText("2.flac");
    await expect(pane(page).getByText("Succeeded", { exact: true })).toBeVisible();
    await expect(pane(page).getByRole("button", { name: "Open", exact: true })).toBeVisible();

    const details = pane(page).getByRole("button", { name: "Details", exact: true });
    const technical = pane(page).getByRole("button", { name: "Technical", exact: true });
    await expect(details).toHaveAttribute("aria-expanded", "true");
    await expect(technical).toHaveAttribute("aria-expanded", "false");

    // an empty field is left out rather than shown as a dash
    await expect(pane(page).getByText("Folder", { exact: true })).toBeVisible();
    await expect(pane(page).getByText("Attempts", { exact: true })).toHaveCount(0);
    await expect(pane(page).getByText("—", { exact: true })).toHaveCount(0);
    // the daemon behind these fakes has no such transfer, which History says rather than waiting
    await expect(pane(page).getByText(/^Could not read the history/)).toBeVisible();

    // the raw values are folded away, each with its copy button
    await technical.click();
    await expect(pane(page).getByText("Completed, Succeeded", { exact: true })).toBeVisible();
    await expect(pane(page).getByText("@@a\\Music\\Two\\2.flac", { exact: true })).toBeVisible();
    await expect(pane(page).getByRole("button", { name: "Copy remote path" })).toBeVisible();

    await details.click();
    await expect(pane(page).getByText("Folder", { exact: true })).toHaveCount(0);

    // each stays as it was left, on another file and after a reload
    await page.getByRole("button", { name: "3.flac", exact: true }).click();
    await expect(fileName(page)).toHaveText("3.flac");
    await expect(details).toHaveAttribute("aria-expanded", "false");
    await expect(technical).toHaveAttribute("aria-expanded", "true");
    await page.reload();
    await expect(pane(page)).toBeVisible();
    await expect(pane(page).getByText("No file selected")).toBeVisible();
    await page.getByRole("button", { name: "2.flac", exact: true }).click();
    await expect(details).toHaveAttribute("aria-expanded", "false");
    await expect(technical).toHaveAttribute("aria-expanded", "true");
  });

  test("shows a search result, and the toolbar opens and closes it", async ({ page }) => {
    await fakeConnected(page);
    await fakeSearch(page);
    await login(page, `/searches/${SEARCH_ID}`);
    await page.getByRole("radio", { name: "Dense" }).click();
    const grid = page.getByRole("grid", { name: "Search results" });
    await expect(grid.getByText("02 Two.flac")).toBeVisible();

    const toggle = page.locator("main").getByRole("button", { name: "Details", exact: true });
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-pressed", "true");
    await expect(pane(page).getByText("No file selected")).toBeVisible();

    await grid.focus();
    await page.keyboard.press("ArrowDown");
    await expect(fileName(page)).toHaveText("01 One.flac");
    await expect(pane(page).getByText("Upload speed", { exact: true })).toBeVisible();
    await expect(pane(page).getByRole("button", { name: "Download", exact: true })).toBeVisible();
    await page.keyboard.press("ArrowDown");
    await expect(fileName(page)).toHaveText("02 Two.flac");

    // the tree shows it too
    await page.getByRole("radio", { name: "Tree" }).click();
    const tree = page.getByRole("treegrid", { name: "Search results" });
    await tree.getByText("03 Three.mp3").click();
    await expect(fileName(page)).toHaveText("03 Three.mp3");

    await toggle.click();
    await expect(pane(page)).toBeHidden();
  });

  test("shows a browsed file", async ({ page }) => {
    await fakeConnected(page);
    await fakeBrowse(page);
    await login(page, "/users/carol/shares");
    await page.locator('[data-line$="Bootlegs"]').click();
    await page.getByRole("treegrid", { name: "Shared folders" }).getByText("2.flac").click();
    await page.keyboard.press("i");
    await expect(fileName(page)).toHaveText("2.flac");
    await expect(pane(page).getByRole("button", { name: "Download", exact: true })).toBeVisible();
  });

  test("leaves the pane out of a screen without one", async ({ page }) => {
    await fakeDownloads(page);
    await login(page, "/downloads");
    await page.getByRole("button", { name: "2.flac", exact: true }).click();
    await expect(pane(page)).toBeVisible();
    await page.goto("./#/dashboard");
    await expect(pane(page)).toHaveCount(0);
    await page.goto("./#/downloads");
    await expect(pane(page)).toBeVisible();
  });
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("opens as a sheet from a row, closes with Escape and stays closed", async ({ page }) => {
    await fakeSearch(page);
    await login(page, `/searches/${SEARCH_ID}`);
    await page.getByRole("button", { name: "Actions for 01 One.flac" }).click();
    await page.getByRole("menuitem", { name: "Details" }).click();
    const sheet = page.getByRole("dialog", { name: "Details" });
    await expect(sheet.getByRole("heading", { name: "01 One.flac" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
    await page.reload();
    await expect(page.getByRole("button", { name: "Actions for 01 One.flac" })).toBeVisible();
    await expect(sheet).toHaveCount(0);
  });
});
