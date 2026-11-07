import { expect, test } from "@playwright/test";

import { fakeChat, frame, helloFrame } from "./support/fakes";
import { refuseCopy, visit } from "./support/login";

test("a room reads like a chat: names, actions, links, the wall and the activity", async ({
  page
}) => {
  await fakeChat(page);
  // the first stream that carries the room's frames has a member join; every later one is the
  // daemon's own
  let served = false;
  await page.route("**/api/v0/live**", (route) => {
    const topics = (new URL(route.request().url()).searchParams.get("topics") ?? "").split(",");
    if (served || !topics.includes("rooms")) return route.continue();
    served = true;
    const joined = frame(2, "rooms.user_joined", {
      roomName: "indie",
      user: { username: "guest", status: "Online", self: false }
    });
    return route.fulfill({
      headers: { "content-type": "text/event-stream" },
      body: helloFrame(topics) + joined
    });
  });
  await visit(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("#indie");
  // bob's two lines are one group under one name, colored the same everywhere
  await expect(log.getByRole("link", { name: "bob", exact: true })).toHaveCount(1);
  const color = await log
    .getByRole("link", { name: "bob", exact: true })
    .evaluate((a) => getComputedStyle(a).color);
  const listed = page.getByRole("grid", { name: "Users in the room" });
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
  await expect(page.getByRole("option", { name: "Join room server" })).toBeVisible();
  await expect(page.getByRole("option", { name: /Start a conversation/ })).toHaveCount(0);
  await page.goto("./#/users/server");
  await expect(page).toHaveURL(/#\/chat\/server$/);
});

test("a conversation's menu blocks the user, the server's notices have no such item", async ({
  page
}) => {
  await fakeChat(page);
  await visit(page, "/chat");
  await page.goto("./#/chat/alice");
  await page.getByRole("button", { name: "Conversation menu" }).click();
  await page.getByRole("menuitem", { name: "Block" }).click();
  await expect(page.getByRole("dialog", { name: /block/i })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.goto("./#/chat/server");
  await page.getByRole("button", { name: "Server notices menu" }).click();
  await expect(page.getByRole("menuitem", { name: "Export as text" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Block" })).toHaveCount(0);
});

test("the keys reach what can be done with a message", async ({ page }) => {
  await fakeChat(page);
  await visit(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  const copy = log.getByRole("button", { name: "Copy the text" });
  await expect(copy).toBeHidden();
  await log.focus();
  await page.keyboard.press("ArrowUp");
  // the keys put focus on the newest message, which shows its toolbar
  const newest = log.locator("[data-message]").last();
  await expect(newest).toBeFocused();
  await expect(copy).toHaveCount(1);
  await expect(copy).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menuitem", { name: "Copy the text" })).toBeVisible();
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

test("a toast sits above the docked activity log", async ({ page }) => {
  await refuseCopy(page);
  await fakeChat(page);
  await visit(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("thanks bob, grabbing it now")).toBeVisible();
  await page.keyboard.press("Control+j");
  const dock = page.getByRole("region", { name: "Activity on the network" });
  await expect.poll(async () => (await dock.boundingBox())?.height).toBe(288);
  await log.getByText("thanks bob, grabbing it now").hover();
  await page.getByRole("button", { name: "Copy the text" }).last().click();
  const toast = page.getByRole("alert").filter({ hasText: "Could not copy" });
  await expect(toast).toBeVisible();
  const [above, top] = [(await toast.boundingBox())!, (await dock.boundingBox())!];
  expect(above.y + above.height).toBeLessThanOrEqual(top.y);
});
