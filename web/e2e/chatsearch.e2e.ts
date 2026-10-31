import { expect, type Page, test } from "@playwright/test";

import { fakeChat } from "./support/fakes";
import { login, textSize } from "./support/login";

const box = (page: Page) => page.getByRole("combobox", { name: "Search chats" });
const options = (page: Page) => page.getByRole("option");
const results = (page: Page) => page.getByRole("grid", { name: "Search results" });

const at = (id: number) => new Date(Date.UTC(2026, 9, 27, 12, 0, id)).toISOString();

/** Message `id` of the long room "jazz", the same on every page */
const jazz = (id: number) => ({
  localId: id,
  timestamp: at(id),
  username: id % 2 ? "bob" : "carol",
  message: id === 150 ? "floyd came up here" : `line ${id}`,
  roomName: "jazz",
  self: false,
  isMention: false,
  hidden: false
});

const HITS = [
  {
    kind: "room",
    room: "jazz",
    username: "carol",
    timestamp: at(150),
    message: "floyd came up here",
    localId: 150,
    snippet: "<mark>floyd</mark> came up here"
  },
  {
    kind: "private",
    room: null,
    username: "alice",
    timestamp: at(1),
    message: "have you heard floyd?",
    localId: 2,
    snippet: "have you heard <mark>floyd</mark>?"
  }
];

/**
 * Adds to `fakeChat`: a long room "jazz" known from the server's list, read a page at a time, a
 * user in the directory, and a search whose messages hold "floyd"
 */
const fakeSearch = async (page: Page) => {
  await fakeChat(page);
  await page.route("**/api/v0/rooms/available", (route) =>
    route.fulfill({
      json: [
        { name: "jazz", userCount: 12, isPrivate: false, isOwned: false, isModerated: false },
        { name: "floydians", userCount: 3, isPrivate: false, isOwned: false, isModerated: false }
      ].map((r) => ({ ...r, joined: false }))
    })
  );
  await page.route("**/api/v0/rooms/history", (route) => route.fulfill({ json: { rooms: [] } }));
  await page.route("**/api/v0/rooms/history/jazz/messages**", (route) => {
    const url = new URL(route.request().url());
    const limit = Number(url.searchParams.get("limit"));
    const ids = Array.from({ length: 450 }, (_, n) => n + 1);
    const after = url.searchParams.get("after");
    const past = ids.filter((id) => id > Number(after));
    const up = ids.filter((id) => id < Number(url.searchParams.get("before") ?? 451));
    const page = after === null ? up.slice(-limit) : past.slice(0, limit);
    const more = (after === null ? up : past).length > limit;
    return route.fulfill({ json: { messages: page.map(jazz), hasMore: more, gaps: [] } });
  });
  await page.route("**/api/v0/rooms/joined/jazz**", (route) =>
    route.fulfill({ status: 404, json: { error: "Not in the room" } })
  );
  await page.route("**/api/v0/users?**", (route) => {
    const q = new URL(route.request().url()).searchParams.get("q") ?? "";
    const users = "floyd_fan".includes(q)
      ? [{ username: "floyd_fan", presence: "Online", tags: [] }]
      : [];
    return route.fulfill({ json: { users, total: users.length } });
  });
  await page.route("**/api/v0/messages/search**", (route) => {
    const q = new URL(route.request().url()).searchParams.get("q") ?? "";
    const hits = q.includes("floyd") || q.includes("flo") ? HITS : [];
    return route.fulfill({ json: { hits, total: hits.length } });
  });
};

test("typing lists rooms, users and messages in categories", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await box(page).fill("floyd");
  const list = page.getByRole("listbox");
  await expect(list).toContainText("Rooms");
  await expect(list).toContainText("Users");
  await expect(list).toContainText("Messages");
  await expect(options(page)).toHaveText([
    /All results for "floyd"/,
    /floydians.*3 users/,
    /floyd_fan.*Online/,
    /#jazz, carol.*floyd came up here/,
    /alice.*have you heard floyd\?/
  ]);
  // what matched is marked, in names and in messages
  await expect(list.locator("mark").first()).toHaveText("floyd");
});

test("an exact name opens its chat from the box, by Enter or a click", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await box(page).fill("indie");
  await expect(options(page).first()).toContainText("indie");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/chat\/rooms\/indie$/);
  await expect(box(page)).toHaveValue("");

  // a name known only to the directory is waited for, however fast the Enter comes
  await box(page).fill("floyd_fan");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/chat\/floyd_fan$/);

  await box(page).fill("alice");
  await options(page).first().click();
  await expect(page).toHaveURL(/#\/chat\/alice$/);
});

test("the arrows reach every entry and Escape closes the list", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await box(page).fill("floyd");
  await expect(options(page)).toHaveCount(5);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(options(page).nth(2)).toHaveAttribute("data-highlighted", "");
  await page.keyboard.press("Escape");
  await expect(options(page)).toHaveCount(0);
  await expect(box(page)).toHaveValue("floyd");
  await page.keyboard.press("ArrowDown");
  await expect(options(page)).toHaveCount(5);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/chat\/rooms\/floydians$/);
});

test("Enter opens the results, a message opens its chat, Back and Escape return", async ({
  page
}) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await box(page).fill("floyd");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/chat\/search\?q=floyd$/);

  // categorized, the messages under their chat, with the keys on the list
  const lines = results(page);
  await expect(lines).toBeFocused();
  await expect(lines.getByRole("columnheader")).toHaveText(["Rooms", "Users", "Messages"]);
  await expect(lines.locator('[data-line="chat:room:jazz"]')).toContainText("jazz");
  await expect(lines.locator("mark").first()).toHaveText("floyd");
  await expect(box(page)).toHaveValue("floyd");

  await page.reload();
  await expect(results(page).getByRole("row").filter({ hasText: "came up here" })).toBeVisible();
  await expect(box(page)).toHaveValue("floyd");

  const hit = results(page).getByRole("row").filter({ hasText: "came up here" });
  await hit.click();
  await expect(page).toHaveURL(/#\/chat\/rooms\/jazz\?at=150&q=floyd$/);

  // the real conversation, around the message, highlighted, with the chat's own composer
  const log = page.getByRole("log");
  const found = log.locator('[data-message="150"]');
  await expect(found).toBeVisible();
  await expect(found).toHaveClass(/ring-accent/);
  await expect(log.locator('[data-message="151"]')).toBeVisible();
  await expect(page.getByRole("textbox", { name: "Message" })).toBeVisible();
  await found.hover();
  await expect(found.getByRole("button", { name: "Copy the text" })).toBeVisible();

  // older and newer pages come as the list scrolls
  await log.evaluate((b) => (b.scrollTop = b.scrollHeight));
  await expect(log.locator('[data-message="251"]')).toBeAttached();
  await log.evaluate((b) => (b.scrollTop = 0));
  await expect(log.locator('[data-message="50"]')).toBeAttached();
  await expect(page.getByRole("button", { name: "Jump to latest" })).toBeVisible();

  await page.goBack();
  await expect(page).toHaveURL(/#\/chat\/search\?q=floyd$/);
  await expect(results(page)).toBeVisible();

  await page.keyboard.press("ArrowDown");
  await results(page).getByRole("row").filter({ hasText: "came up here" }).click();
  await expect(log.locator('[data-message="150"]')).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page).toHaveURL(/#\/chat\/search\?q=floyd$/);
  await expect(box(page)).toHaveValue("floyd");
  await expect(results(page)).toBeVisible();

  // the same way back from the bar over the chat
  await results(page).getByRole("row").filter({ hasText: "have you heard" }).click();
  await expect(page).toHaveURL(/#\/chat\/alice\?at=2&q=floyd$/);
  await page.getByRole("button", { name: 'Back to the results for "floyd"' }).click();
  await expect(results(page)).toBeVisible();
});

test("Jump to latest leaves the older messages for the newest", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/jazz?at=150&q=floyd");
  const log = page.getByRole("log");
  await expect(log.locator('[data-message="150"]')).toBeVisible();
  await page.getByRole("button", { name: "Jump to latest" }).click();
  await expect(log.locator('[data-message="450"]')).toBeVisible();
  await expect(log.locator('[data-message="150"]')).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Jump to latest" })).toHaveCount(0);
});

test("a search that finds nothing says so, and offers the name", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await box(page).fill("zzz");
  await expect(page.getByRole("listbox")).toContainText("No rooms, users or messages match.");
  await page.keyboard.press("Enter");
  await expect(page.getByText('Nothing matches "zzz"')).toBeVisible();
  await page.getByRole("button", { name: "Join this room" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("a long category shows its first rooms until all are asked for", async ({ page }) => {
  await fakeSearch(page);
  await page.route("**/api/v0/rooms/available", (route) =>
    route.fulfill({
      json: Array.from({ length: 25 }, (_, n) => ({
        name: `floyd${n}`,
        userCount: 1,
        isPrivate: false,
        isOwned: false,
        isModerated: false,
        joined: false
      }))
    })
  );
  await login(page, "/chat");
  await page.goto("./#/chat/search?q=floyd");
  const rooms = results(page).locator('[data-line^="room:"]');
  await expect(rooms).toHaveCount(20);
  await results(page).getByRole("row", { name: "Show all 25 rooms" }).click();
  await expect(rooms).toHaveCount(25);
});

test("the results with no search say what to do", async ({ page }) => {
  await fakeSearch(page);
  await login(page, "/chat");
  await page.goto("./#/chat/search");
  await expect(page.getByText("Search rooms, users and messages")).toBeVisible();
});

test("a query the daemon can't read finds the words", async ({ page }) => {
  await fakeSearch(page);
  await page.route("**/api/v0/messages/search**", (route) => {
    const q = new URL(route.request().url()).searchParams.get("q") ?? "";
    if (q === '"floyd')
      return route.fulfill({ status: 400, json: { error: "not a valid search" } });
    return route.fulfill({ json: { hits: q === '"floyd"*' ? HITS : [], total: 2 } });
  });
  await login(page, "/chat");
  await box(page).fill('"floyd');
  await page.keyboard.press("Enter");
  await expect(results(page).getByRole("row").filter({ hasText: "came up here" })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("the box works from the sheet and the results fit the screen", async ({ page }) => {
    await textSize(page, 24);
    await fakeSearch(page);
    await login(page, "/chat");
    await box(page).fill("floyd");
    await expect(options(page)).toHaveCount(5);
    const list = (await page.getByRole("listbox").boundingBox())!;
    expect(list.x).toBeGreaterThanOrEqual(0);
    expect(list.x + list.width).toBeLessThanOrEqual(390);
    // the list scrolls inside the screen, so the last entry can be reached
    await options(page).last().scrollIntoViewIfNeeded();
    await expect(options(page).last()).toBeInViewport();
    await page.keyboard.press("Enter");
    await expect(results(page)).toBeVisible();
    const wide = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth
    );
    expect(wide).toBe(0);

    await results(page).getByRole("row").filter({ hasText: "came up here" }).click();
    await expect(page.getByRole("log").locator('[data-message="150"]')).toBeVisible();
    await page.getByRole("button", { name: 'Back to the results for "floyd"' }).click();
    await expect(results(page)).toBeVisible();

    // from a chat the box is in the sheet
    await page.goto("./#/chat/alice");
    await page.getByRole("button", { name: "Chats" }).click();
    await box(page).fill("floyd");
    await expect(options(page)).toHaveCount(5);
    await options(page).nth(1).click();
    await expect(page).toHaveURL(/#\/chat\/rooms\/floydians$/);
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
});
