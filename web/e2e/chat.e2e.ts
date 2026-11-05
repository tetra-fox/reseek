import { expect, test } from "@playwright/test";

import { fakeChat, fakeConnected, frame, helloFrame } from "./support/fakes";
import { login, refuseCopy } from "./support/login";

test("a room reads like a chat: names, actions, links and the wall", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/room.png` });
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
  // a message's text is for reading and copying: the I-beam; the name above it is a link
  await expect(log.getByText("320 too if you want it smaller")).toHaveCSS("cursor", "text");
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

  // a click on a name opens their card and keeps it open
  await log.getByRole("link", { name: "alice", exact: true }).first().click();
  await expect(page.getByRole("dialog", { name: "Profile of alice" })).toBeVisible();
  await page.mouse.move(5, 5);
  await page.waitForTimeout(400);
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
  await login(page, "/chat");
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
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/notices.png` });

  // the reserved name is offered as a room, never as a user
  const search = page.getByRole("combobox", { name: "Search chats" });
  await search.fill("server");
  await expect(page.getByRole("option", { name: "Join room server" })).toBeVisible();
  await expect(page.getByRole("option", { name: /Start a conversation/ })).toHaveCount(0);
  await page.goto("./#/users/server");
  await expect(page).toHaveURL(/#\/chat\/server$/);
});

test("New message picks a known user with the pointer and refuses server", async ({ page }) => {
  await fakeChat(page);
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/users"),
    (route) =>
      route.fulfill({
        json: {
          users: [
            {
              username: "alice",
              known: true,
              group: { name: "default", source: "builtin", priority: 1, color: null },
              tags: [],
              lastSeenAt: null
            }
          ],
          total: 1
        }
      })
  );
  await login(page, "/chat");
  await page.getByRole("button", { name: "New message" }).click();
  const dialog = page.getByRole("dialog", { name: "New message" });
  const name = dialog.getByRole("combobox", { name: "Username" });
  await expect(name).toBeFocused();
  await name.fill("server");
  await expect(dialog.getByText("server is not a user")).toBeVisible();
  await name.fill("ali");
  const entry = page.getByRole("option", { name: /alice/ });
  await entry.click();
  // the dialog stays, the name is taken and the message is next
  await expect(name).toHaveValue("alice");
  await expect(entry).toBeHidden();
  await expect(dialog.getByRole("textbox", { name: "Message" })).toBeFocused();
});

test("a conversation's menu blocks the user, the server's notices have no such item", async ({
  page
}) => {
  await fakeChat(page);
  await login(page, "/chat");
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

test("a busy room's comings and goings stay out of its messages", async ({ page }) => {
  await fakeChat(page);
  // the first stream that carries the room's frames is a burst of 120 members joining and
  // leaving; every later one is the daemon's own
  let served = false;
  await page.route("**/api/v0/live**", (route) => {
    const topics = new URL(route.request().url()).searchParams.get("topics") ?? "";
    if (served || !topics.split(",").includes("rooms")) return route.continue();
    served = true;
    const hello = helloFrame(topics.split(","));
    const churn = Array.from({ length: 120 }, (_, n) =>
      n % 2
        ? frame(n + 2, "rooms.user_left", { roomName: "indie", username: `guest${n - 1}` })
        : frame(n + 2, "rooms.user_joined", {
            roomName: "indie",
            user: { username: `guest${n}`, status: "Online", self: false }
          })
    );
    return route.fulfill({
      headers: { "content-type": "text/event-stream" },
      body: hello + churn.join("")
    });
  });
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  await page.getByRole("tab", { name: "Activity" }).click();
  const activity = page.getByRole("grid", { name: "Joins, leaves and status changes" });
  await expect(activity.getByRole("row").first()).toContainText(/\bleft\b/);
  await expect(activity.getByRole("row")).toHaveCount(120);
  // a row's time keeps to one line, as in the activity log
  const time = (await activity.getByRole("row").first().locator("time").boundingBox())!;
  expect(time.height).toBeLessThan(20);
  // the message list holds the six messages and nothing else
  await expect(log.locator("[data-message]")).toHaveCount(6);
  await expect(log).not.toContainText("joined");
  await expect(log).not.toContainText(" left");
});

test("the keys reach what can be done with a message", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/chat");
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

test("a private message stays on one line", async ({ page }) => {
  await fakeConnected(page);
  await fakeChat(page);
  await login(page, "/chat");
  await page.goto("./#/chat/alice");
  const box = page.getByRole("textbox", { name: "Message" });
  await box.click();
  await page.keyboard.type("one");
  await page.keyboard.press("Shift+Enter");
  await page.keyboard.type("two");
  await expect(box).toHaveValue("onetwo");

  await box.fill("");
  await page.keyboard.insertText("pasted\nlines");
  const note = page.getByText("Line breaks are replaced with spaces in private messages.");
  await expect(note).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(note).toHaveCount(0);
  await expect(page.getByRole("log").getByText("pasted lines")).toBeVisible();
});

test("a room's message keeps its line breaks", async ({ page }) => {
  await fakeConnected(page);
  await fakeChat(page);
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const box = page.getByRole("textbox", { name: "Message" });
  await box.click();
  await page.keyboard.type("one");
  await page.keyboard.press("Shift+Enter");
  await page.keyboard.type("two");
  await expect(box).toHaveValue("one\ntwo");
  await expect(page.getByText("Line breaks are replaced")).toHaveCount(0);
  await box.fill("");
});

test("a toast sits above the docked activity log", async ({ page }) => {
  await refuseCopy(page);
  await fakeChat(page);
  await login(page, "/chat");
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
