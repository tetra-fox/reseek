import { expect, type Page, test } from "@playwright/test";

import { login } from "./login";

const ID = "6f1c9a52-0d1e-4c43-9d55-3a1f0b6e7a10";

const file = (filename: string, size = 4_000_000) => ({
  filename,
  size,
  code: 1,
  extension: filename.split(".").pop(),
  isLocked: false,
  attributeCount: 0,
  attributes: [],
  bitRate: 320,
  length: 200
});

const response = (username: string, speed: number, files: string[]) => ({
  username,
  country: "NL",
  token: 1,
  hasFreeUploadSlot: true,
  uploadSpeed: speed,
  queueLength: 0,
  fileCount: files.length,
  files: files.map((f) => file(f)),
  lockedFileCount: 0,
  lockedFiles: []
});

/** Answers the search page's calls with one finished search and its responses */
const fakeSearch = async (page: Page) => {
  const responses = [
    response("alice", 3_000_000, [
      "@@a\\Music\\Album\\01 One.flac",
      "@@a\\Music\\Album\\02 Two.flac",
      "@@a\\Music\\Other\\03 Three.mp3"
    ]),
    response("bob", 1_000_000, ["music/x/1.flac", "music/x/2.flac"])
  ];
  await page.route(`**/api/v0/searches/${ID}**`, (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/responses")) return route.fulfill({ json: responses });
    return route.fulfill({
      json: {
        id: ID,
        searchText: "album",
        token: 1,
        state: "Completed, TimedOut",
        isComplete: true,
        startedAt: "2026-10-26T10:00:00Z",
        endedAt: "2026-10-26T10:00:15Z",
        responseCount: 2,
        fileCount: 5,
        lockedFileCount: 0,
        responseBytes: 0,
        responses: null,
        scope: { type: "network", subjects: [] }
      }
    });
  });
};

test("shows search results as cards and as dense rows walked by key", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/dashboard");
  await page.goto(`./#/searches/${ID}`);
  const crumbs = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(crumbs).toContainText("Search");
  await expect(crumbs).toContainText("album");

  // cards: one per user, folders as a tree under the chain they share
  const alice = page.locator('section[data-key="0:alice"]');
  await expect(alice.getByRole("button", { name: "Close @@a\\Music" })).toBeVisible();
  await expect(alice.getByText("01 One.flac")).toBeVisible();
  // Alt-click closes the folder and the one beside it
  await alice.getByRole("button", { name: "Close Album" }).click({ modifiers: ["Alt"] });
  await expect(alice.getByText("01 One.flac")).toBeHidden();
  await expect(alice.getByText("03 Three.mp3")).toBeHidden();
  await alice.getByRole("button", { name: "Open Album" }).click({ modifiers: ["Alt"] });
  await expect(alice.getByText("01 One.flac")).toBeVisible();
  await expect(alice.getByText("03 Three.mp3")).toBeVisible();

  // a filter token keeps only what it matches, and the clear button empties the box
  const filter = page.getByLabel("Filter", { exact: true });
  await filter.fill("type:mp3");
  await expect(alice.getByText("03 Three.mp3")).toBeVisible();
  await expect(page.locator('section[data-key="0:bob"]')).toHaveCount(0);
  await page.getByRole("button", { name: "Clear" }).click();
  await expect(filter).toHaveValue("");
  await expect(page.locator('section[data-key="0:bob"]')).toBeVisible();

  // the dense view: one row per file, walked with the keys
  await page.getByRole("radio", { name: "Dense" }).click();
  const grid = page.getByRole("grid", { name: "Search results" });
  await expect(grid.locator("[role=row][data-line]")).toHaveCount(5);
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

test("groups downloads by user and a tree of folders", async ({ page }) => {
  const transfer = (filename: string, state = "Completed, Succeeded") => ({
    id: crypto.randomUUID(),
    username: "alice",
    direction: "Download",
    filename,
    size: 1000,
    state,
    requestedAt: "2026-10-26T10:00:00Z",
    bytesTransferred: state === "InProgress" ? 400 : 1000,
    bytesRemaining: 0,
    averageSpeed: state === "InProgress" ? 100 : 0,
    percentComplete: state === "InProgress" ? 40 : 100,
    attempts: 1,
    removed: false
  });
  const files = [
    transfer("@@a\\Music\\One\\1.flac", "InProgress"),
    transfer("@@a\\Music\\Two\\2.flac")
  ];
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
    (route) =>
      route.fulfill({
        json: [{ username: "alice", directories: [{ directory: "d", fileCount: 2, files }] }]
      })
  );
  await login(page, "/downloads");
  const grid = page.getByRole("grid", { name: "Downloads" });
  await expect(grid.getByRole("button", { name: "Close @@a\\Music" })).toBeVisible();
  // the folder with a running download shows how far it is
  await expect(grid.getByText("70%").first()).toBeVisible();
  await grid.getByRole("button", { name: "Close One" }).click({ modifiers: ["Alt"] });
  await expect(grid.getByText("1.flac")).toBeHidden();
  await expect(grid.getByText("2.flac")).toBeHidden();
});

test("browses a user, finds a folder by typing and shows where it is", async ({ page }) => {
  const dir = (name: string, files: string[]) => ({
    name,
    fileCount: files.length,
    files: files.map((f) => file(f))
  });
  // carol is answered here alone, never asked of the daemon, so no other test sees her
  await page.route(
    (url) => url.pathname.includes("/api/v0/users/carol/"),
    (route) =>
      !new URL(route.request().url()).pathname.endsWith("/carol/browse")
        ? route.fulfill({ status: 404, json: { error: "not found" } })
        : route.fulfill({
            json: {
              directories: [
                dir("@@c\\Music", []),
                dir("@@c\\Music\\Albums", ["1.flac"]),
                dir("@@c\\Music\\Bootlegs", ["2.flac"])
              ],
              directoryCount: 3,
              lockedDirectories: [],
              lockedDirectoryCount: 0
            }
          })
  );
  await login(page, "/browse");
  await page.getByLabel("Username").fill("carol");
  await page.getByLabel("Username").press("Enter");
  const tree = page.getByRole("tree", { name: "Shared folders" });
  await expect(tree.getByText("Bootlegs")).toBeVisible();
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
