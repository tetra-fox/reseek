import { expect, test } from "@playwright/test";

import { frameText } from "../src/lib/testing/frames";
import { fakeChat, helloFrame } from "./support/fakes";
import { open, refuseCopy, visit } from "./support/login";

test("a room reads like a chat: names, actions, links, the wall and the activity", async ({
  page
}) => {
  await fakeChat(page);
  // the daemon's own stream would list no joined rooms, and a pane keeps only the joins made
  // while it is open: the streams wait for the pane, and the first has a member join
  let paneOpen!: () => void;
  const opened = new Promise<void>((resolve) => (paneOpen = resolve));
  let joinSent = false;
  await page.route("**/api/v0/live**", async (route) => {
    const topics = (new URL(route.request().url()).searchParams.get("topics") ?? "").split(",");
    if (!topics.includes("rooms")) return route.continue();
    await opened;
    const joined = frameText(2, "rooms.user_joined", {
      roomName: "indie",
      user: { username: "guest", status: "Online", self: false }
    });
    const body = helloFrame(topics) + (joinSent ? "" : joined);
    joinSent = true;
    return route.fulfill({ headers: { "content-type": "text/event-stream" }, body });
  });
  await visit(page, "/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  paneOpen();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("#indie");
  // bob's two lines are one group under one name, colored the same everywhere
  await expect(log.getByRole("link", { name: "bob", exact: true })).toHaveCount(1);
  const color = await log
    .getByRole("link", { name: "bob", exact: true })
    .evaluate((a) => getComputedStyle(a).color);
  const listed = page.getByRole("grid", { name: "Room members" });
  await expect(listed.getByRole("link", { name: "bob", exact: true })).toHaveCSS("color", color);
  // a name takes only its text's width: its menu button takes no room until it shows
  const widths = await log
    .getByRole("link", { name: "bob", exact: true })
    .evaluate((a) => [
      a.getBoundingClientRect().width,
      a.parentElement!.getBoundingClientRect().width
    ]);
  expect(widths[1]).toBeCloseTo(widths[0], 0);
  // a /me message is an action, a slsk:// link opens the shares, a mention is marked
  await expect(log.getByText("waves at everyone")).toBeVisible();
  await expect(log.getByRole("link", { name: /^slsk:\/\/bob/ })).toHaveAttribute(
    "href",
    /#\/users\/bob\/shares\?path=Music/
  );
  await expect(log.locator('[data-message="6"]')).toHaveClass(/border-warning/);

  // the member list scrolls inside its panel
  const box = await listed.boundingBox();
  const panel = await page.getByRole("complementary", { name: "Room users" }).boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(panel!.y + panel!.height + 1);

  await page.getByRole("tab", { name: /Wall/ }).click();
  await expect(page.getByRole("region", { name: "Room wall" })).toContainText(
    "listening to Geogaddi"
  );
  // a row of the room's activity keeps its time to one line, as in the activity log
  await page.getByRole("tab", { name: "Activity" }).click();
  const activity = page.getByRole("grid", { name: "Joins, leaves and status changes" });
  await expect(activity.getByRole("row").first()).toContainText(/\bjoined\b/);
  const time = (await activity.getByRole("row").first().locator("time").boundingBox())!;
  expect(time.height).toBeLessThan(20);

  // a click on a name opens their card and keeps it open
  await log.getByRole("link", { name: "alice", exact: true }).first().click();
  await expect(page.getByRole("dialog", { name: "Profile of alice" })).toBeVisible();
  await page.mouse.move(5, 5);
  // the card closes 200 ms after the pointer leaves, so a page timer as long fires after that one
  await page.evaluate(() => new Promise((done) => setTimeout(done, 200)));
  await expect(page.getByRole("dialog", { name: "Profile of alice" })).toBeVisible();
  // the card's More opens the name's menu, which no button beside the name offers
  await page
    .getByRole("dialog", { name: "Profile of alice" })
    .getByRole("button", { name: "More" })
    .click();
  await expect(page.getByRole("menuitem", { name: "Copy username" })).toBeVisible();
  await page.keyboard.press("Escape");
});

test("a slsk:// link's tooltip names its user as every username looks", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat/rooms/indie");
  const log = page.getByRole("log");
  const color = await log
    .getByRole("link", { name: "bob", exact: true })
    .evaluate((a) => getComputedStyle(a).color);
  await log.getByRole("link", { name: /^slsk:\/\/bob/ }).hover();
  const tip = page.locator("[data-tooltip-content]");
  await expect(tip).toHaveText("Browse folder in bob's shares");
  await expect(tip.getByText("bob", { exact: true })).toHaveCSS("color", color);
});

test("the server's notices are one read-only entry, reached by the keys", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat");
  const list = page.getByRole("grid", { name: "Rooms and conversations" });
  const lines = list.locator("[data-line]");
  await expect(lines.last()).toHaveAttribute("data-line", "user:server");
  await expect(lines.nth(-2)).toContainText("Notices");
  await expect(lines.last().getByRole("link")).toHaveCount(0);
  await lines.last().getByRole("button", { name: "Menu for Server" }).focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Export as text",
    "Export as JSON",
    "Clear history"
  ]);
  await page.keyboard.press("Escape");
  await expect(lines.last().getByRole("button", { name: "Menu for Server" })).toBeFocused();

  await list.focus();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#\/chat\/server$/);
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("Server");
  await expect(page.getByRole("heading", { name: "Server" })).toBeVisible();
  const log = page.getByRole("log");
  await expect(log.getByText("is now a member of room")).toBeVisible();
  await expect(log.getByRole("link")).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Message" })).toHaveCount(0);
  await expect(log.locator("[data-message]").first().locator("time")).toBeVisible();

  // the reserved name is offered as a room, never as a user
  const search = page.getByRole("combobox", { name: "Search chats" });
  await search.fill("server");
  await expect(page.getByRole("option", { name: "Join server" })).toBeVisible();
  await expect(page.getByRole("option", { name: "Message server" })).toHaveCount(0);
  await open(page, "/users/server");
  await expect(page).toHaveURL(/#\/chat\/server$/);
});

test("a conversation's menu blocks the user, the server's notices have no such item", async ({
  page
}) => {
  await fakeChat(page);
  await visit(page, "/chat");
  await open(page, "/chat/alice");
  await page.getByRole("button", { name: "Conversation menu" }).click();
  await page.getByRole("menuitem", { name: "Block" }).click();
  await expect(page.getByRole("dialog", { name: /block/i })).toBeVisible();
  await page.keyboard.press("Escape");
  await open(page, "/chat/server");
  await page.getByRole("button", { name: "Server notices menu" }).click();
  await expect(page.getByRole("menuitem", { name: "Export as text" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Block" })).toHaveCount(0);
});

test("the keys reach what can be done with a message", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat");
  await open(page, "/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  const copy = log.getByRole("button", { name: "Copy text" });
  await expect(copy).toBeHidden();
  await log.focus();
  await page.keyboard.press("ArrowUp");
  // the keys put focus on the newest message, which shows its toolbar
  const newest = log.locator("[data-message]").last();
  await expect(newest).toBeFocused();
  await expect(copy).toHaveCount(1);
  await expect(copy).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menuitem", { name: "Copy text" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(newest).toBeFocused();
  // Tab goes from the message to its own toolbar, which comes after its text, and back
  await page.keyboard.press("Tab");
  const first = newest.getByRole("group", { name: "Message actions" }).getByRole("button").first();
  await expect(first).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(newest).toBeFocused();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Escape");
  await expect(newest).toBeFocused();
});

test("a chat row shows its menu is open while the pointer is on the menu", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat");
  const row = page.locator('[data-line="room:indie"]');
  await row.hover();
  const button = row.getByRole("button", { name: "Menu for indie" });
  await button.click();
  await page.getByRole("menuitem").first().hover();
  await expect(button).toHaveCSS("opacity", "1");
  await expect(row).not.toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
});

test("a right-click opens a message's menu at the pointer, as on every list", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat");
  await open(page, "/chat/rooms/indie");
  const text = page.getByRole("log").getByText("anyone have the new boards of canada rip?");
  const box = (await text.boundingBox())!;
  const at = { x: box.x + box.width / 4, y: box.y + box.height / 2 };
  await page.mouse.click(at.x, at.y, { button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  // the menu's corner is at the pointer, below it by the menu's offset
  await expect.poll(async () => (await menu.boundingBox())?.x).toBeCloseTo(at.x, 0);
  const top = (await menu.boundingBox())!.y;
  expect(top - at.y).toBeGreaterThanOrEqual(0);
  expect(top - at.y).toBeLessThan(8);
});

test("a message takes the page's own hover tint", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat");
  await open(page, "/chat/rooms/indie");
  const message = page.getByRole("log").locator("[data-message]").last();
  await message.hover();
  // a line re-points the tones of what is inside it, which would darken the message's own
  const tone = (e: Element) => getComputedStyle(e).getPropertyValue("--color-hover");
  expect(await message.evaluate(tone)).toBe(await page.locator("html").evaluate(tone));
});

test("a toast sits above the docked activity log", async ({ page }) => {
  await refuseCopy(page);
  await fakeChat(page);
  await visit(page, "/chat");
  await open(page, "/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("thanks bob, grabbing it now")).toBeVisible();
  await page.keyboard.press("Control+j");
  const dock = page.getByRole("region", { name: "Activity on the network" });
  await expect.poll(async () => (await dock.boundingBox())?.height).toBe(288);
  await log.getByText("thanks bob, grabbing it now").hover();
  await page.getByRole("button", { name: "Copy text" }).last().click();
  const toast = page.getByRole("alert").filter({ hasText: "Could not copy" });
  await expect(toast).toBeVisible();
  const [above, top] = [(await toast.boundingBox())!, (await dock.boundingBox())!];
  expect(above.y + above.height).toBeLessThanOrEqual(top.y);
});

test("an address typed over a chat asks nothing of its name", async ({ page }) => {
  await fakeChat(page);
  const asked: string[] = [];
  page.on("request", (request) => asked.push(new URL(request.url()).pathname));
  await visit(page, "/chat");
  await page.goto("./#/chat/alice");
  await expect(page.getByRole("log")).toBeVisible();
  // as typed into the browser's bar, which the app reloads for
  await page.goto("./#/system/settings");
  await expect(page.locator('[id="soulseek.description"]')).toBeVisible();
  expect(asked.filter((path) => /conversations\/(system|settings)/.test(path))).toEqual([]);
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  test("a first long press on a message's time opens its menu", async ({ page }) => {
    await fakeChat(page);
    await visit(page, "/chat");
    await open(page, "/chat/rooms/indie");
    const log = page.getByRole("log");
    await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
    const time = log.locator("[data-message] time:visible").last();
    const box = (await time.boundingBox())!;
    const point = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    // held past the long press, which `tap` is not
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [point] });
    await page.waitForTimeout(800);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await expect(page.getByRole("menuitem", { name: "Copy text" })).toBeVisible();
  });
});
