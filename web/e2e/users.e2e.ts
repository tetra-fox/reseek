import { expect, test, type Page, type Route } from "@playwright/test";

import { fakeConnected, user } from "./support/fakes";
import { textSize, visit } from "./support/login";

const alice = user("alice");

const searchBox = (page: Page) => page.getByRole("combobox", { name: "Search users" });
const options = (page: Page) => page.getByRole("option");

/** Answers the directory with `users`, those whose name or tags hold the text searched for */
const directory = (page: Page, users: (typeof alice)[]) =>
  page.route("**/api/v0/users?**", (route) => {
    const q = new URL(route.request().url()).searchParams.get("q") ?? "";
    const matching = users.filter((u) => `${u.username} ${u.tags.join(" ")}`.includes(q));
    return route.fulfill({ json: { users: matching, total: matching.length } });
  });

/** Answers `name`'s profile page, and the look-up of them with `refreshed` */
const profile = (page: Page, name: string, refreshed: (route: Route) => Promise<void>) =>
  page.route(`**/api/v0/users/${name}/**`, (route) =>
    new URL(route.request().url()).searchParams.get("refresh") === "true"
      ? refreshed(route)
      : route.fulfill({ json: { ...alice, username: name } })
  );

test("a country's flag is a real image, served from the build under the URL base", async ({
  page
}) => {
  await profile(page, "alice", (route) => route.fulfill({ json: alice }));
  await visit(page, "/users/alice");
  const flag = page.getByRole("img", { name: "Netherlands" });
  await expect(flag).toBeVisible();
  await expect
    .poll(() => flag.evaluate((image: HTMLImageElement) => image.naturalWidth))
    .not.toBe(0);
});

test("the search box lists users best first, picks one, and closes on Escape and Clear", async ({
  page
}) => {
  await directory(page, [
    { ...alice, username: "alice", tags: ["bob"] },
    { ...alice, username: "bobby" },
    { ...alice, username: "bob", presence: "Online" },
    { ...alice, username: "carol", tags: [] }
  ]);
  await profile(page, "bobby", (route) => route.fulfill({ json: alice }));
  await profile(page, "bob", (route) => route.fulfill({ json: alice }));
  await visit(page, "/users");

  await searchBox(page).fill("bob");
  await expect(options(page)).toHaveText([/\bbob\b.*Online.*default.*Last seen/, /\bbobby\b/]);
  // the list under the box filters too, by tags as well, and the look-up entry stays away from
  // a name the directory holds
  await expect(page.locator('[data-line="alice"]')).toBeVisible();
  await expect(page.locator('[data-line="carol"]')).toHaveCount(0);

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/users\/bobby$/);

  await page.goBack();
  await searchBox(page).fill("bob");
  await expect(options(page)).toHaveCount(2);
  // Escape closes the dropdown and keeps the text, and a click on the box brings it back
  await page.keyboard.press("Escape");
  await expect(options(page)).toHaveCount(0);
  await expect(searchBox(page)).toHaveValue("bob");
  await searchBox(page).click();
  await expect(options(page)).toHaveCount(2);
  // the clear button empties it, and the list under it is whole again
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(searchBox(page)).toHaveValue("");
  await expect(options(page)).toHaveCount(0);
  await expect(page.locator('[data-line="carol"]')).toBeVisible();

  await searchBox(page).fill("bob");
  await options(page).first().click();
  await expect(page).toHaveURL(/#\/users\/bob$/);
});

test("looking up a user asks the network as typed, and says when there is none", async ({
  page
}) => {
  await fakeConnected(page);
  await directory(page, [alice]);
  await profile(page, "nobody", (route) =>
    route.fulfill({ status: 404, json: { error: "User nobody does not exist" } })
  );
  let answer = () => {};
  const answered = new Promise<void>((resolve) => (answer = resolve));
  await profile(page, "NewUser", async (route) => {
    await answered;
    await route.fulfill({ json: { ...alice, username: "NewUser", known: false } });
  });
  await visit(page, "/users");

  // a user who does not exist is said so, and nothing opens
  await searchBox(page).fill("nobody");
  await page.getByRole("option", { name: "Look up nobody" }).click();
  await expect(page.getByText("No user named nobody")).toBeVisible();
  await expect(page).toHaveURL(/#\/users$/);
  await expect(page.getByRole("option", { name: "Look up nobody" })).toBeVisible();

  await searchBox(page).fill("NewUser");
  const entry = options(page).last();
  await expect(entry).toHaveText("Look up NewUser");
  await page.keyboard.press("Enter");
  // the dropdown stays while the network is asked
  await expect(entry).toContainText("Looking up NewUser");
  answer();
  await expect(page).toHaveURL(/#\/users\/NewUser$/);
});

test("a look-up the user walked away from opens nothing", async ({ page }) => {
  await fakeConnected(page);
  await directory(page, [alice]);
  let answer = () => {};
  const answered = new Promise<void>((resolve) => (answer = resolve));
  const asked = page.waitForResponse(/\/users\/slowpoke\/profile\?.*refresh=true/);
  await profile(page, "slowpoke", async (route) => {
    await answered;
    await route.fulfill({ json: { ...alice, username: "slowpoke" } });
  });
  await visit(page, "/users");

  await searchBox(page).fill("slowpoke");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("option", { name: /Looking up slowpoke/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(options(page)).toHaveCount(0);
  answer();
  await asked;
  await expect(page).toHaveURL(/#\/users$/);
});

test("the dropdown fits a phone at the largest text size", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 640 });
  await textSize(page, 24);
  await fakeConnected(page);
  const long = "a-name-as-long-as-they-can-be-0";
  await directory(page, [
    {
      ...alice,
      username: long,
      group: { name: "a group with a long name", source: "config", priority: 1, color: null }
    }
  ]);
  await visit(page, "/users");
  await searchBox(page).fill("a-name");
  const entries = options(page);
  await expect(entries).toHaveCount(2);
  for (const entry of await entries.all()) {
    const box = (await entry.boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(360);
  }
  const wide = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth
  );
  expect(wide).toBe(0);
  // the last entry is reached from the keyboard, inside the dropdown's scroll
  await page.keyboard.press("ArrowDown");
  await expect(entries.last()).toBeInViewport();
});

test("a user's card and row offer Message, Browse shares and Add buddy", async ({ page }) => {
  let buddy: { trusted: boolean; notifyPresence: boolean } | null = null;
  await page.route("**/api/v0/users?**", (route) =>
    route.fulfill({ json: { users: [{ ...alice, buddy }], total: 1 } })
  );
  await page.route("**/api/v0/users/alice/**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/buddy")) {
      const added = route.request().method() === "PUT";
      buddy = added ? { trusted: false, notifyPresence: false } : null;
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ json: { ...alice, buddy } });
  });
  await visit(page, "/users");
  const row = page.locator('[data-line="alice"]');
  await expect(row).toBeVisible();
  // the card on hover, with the actions as buttons and when they were last seen
  await row.getByRole("link", { name: "alice", exact: true }).hover();
  const card = page.getByRole("dialog", { name: "Profile of alice" });
  await expect(card).toBeVisible();
  await expect(card).toContainText("last seen");
  await expect(card.getByRole("link", { name: "Message" })).toBeVisible();
  await expect(card.getByRole("link", { name: "Browse shares" })).toBeVisible();
  await card.getByRole("button", { name: "Add buddy" }).click();
  await expect(card.getByRole("button", { name: "Remove buddy" })).toBeVisible();
  await page.mouse.move(5, 5);
  // the row has the same action
  await expect(row.getByRole("button", { name: /buddy/ })).toBeVisible();
});

test("notes save as you type and say so with a tick", async ({ page }) => {
  let saved = "";
  await page.route("**/api/v0/users/alice/**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/notes")) {
      saved = route.request().postDataJSON().notes;
      return route.fulfill({ status: 204 });
    }
    return route.fulfill({ json: { ...alice, notes: saved, figures: alice.figures } });
  });
  await visit(page, "/users/alice");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("alice");
  await page.getByLabel("Notes").fill("shares rare jazz");
  await expect(page.getByRole("status").filter({ hasText: "Saved" })).toBeVisible();
  expect(saved).toBe("shares rare jazz");
});
