import { expect, type Page, test } from "@playwright/test";

import { login } from "./login";

const at = (minute: number, second = 0) =>
  new Date(Date.UTC(2026, 9, 27, 18, minute, second)).toISOString();

const MEMBERS = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];

/** Answers the chat calls with one room, "indie", and one conversation, with "alice" */
const fakeChat = async (page: Page) => {
  const said: [string, string][] = [
    ["alice", "anyone have the new boards of canada rip?"],
    ["bob", "yes, slsk://bob/Music/Boards%20of%20Canada/Tomorrow's%20Harvest/ in flac"],
    ["bob", "320 too if you want it smaller"],
    ["carol", "/me waves at everyone"],
    ["reseek", "thanks bob, grabbing it now"],
    ["dave", "reseek: did you see https://example.com/review ?"]
  ];
  const roomMessages = said.map(([username, message], n) => ({
    localId: n + 1,
    timestamp: at(n),
    username,
    message,
    roomName: "indie",
    self: username === "reseek",
    isMention: message.includes("reseek"),
    hidden: false
  }));
  const joined = [
    {
      name: "indie",
      unread: 0,
      mentions: 0,
      notify: "mentions",
      userCount: MEMBERS.length,
      lastMessageAt: at(5),
      lastReadId: 6
    }
  ];
  await page.route("**/api/v0/rooms/joined", (route) => route.fulfill({ json: joined }));
  // a pane opened before the joined rooms are read asks the history for the same page
  await page.route("**/api/v0/rooms/history/indie/messages**", (route) =>
    route.fulfill({ json: { messages: roomMessages, hasMore: false, gaps: [] } })
  );
  await page.route("**/api/v0/rooms/joined/indie**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/messages")) {
      return route.fulfill({ json: { messages: roomMessages, hasMore: false, gaps: [] } });
    }
    return route.fulfill({
      json: {
        name: "indie",
        isPrivate: false,
        users: MEMBERS.map((username, n) => ({
          username,
          status: n % 3 === 2 ? "Away" : "Online",
          averageSpeed: 400_000 * (n + 1),
          uploadCount: n,
          fileCount: 1000 * (n + 1),
          directoryCount: 40,
          countryCode: ["NL", "DE", "US", "FR"][n % 4],
          slotsFull: false,
          self: false
        })),
        tickers: [
          { username: "alice", ticker: "listening to Geogaddi" },
          { username: "bob", ticker: "sharing the whole BoC catalogue" }
        ],
        messages: null
      }
    });
  });
  const conversation = {
    username: "alice",
    isActive: true,
    isServerNotice: false,
    unAcknowledgedMessageCount: 0,
    hasUnAcknowledgedMessages: false,
    notify: "all",
    draft: "",
    lastMessage: null,
    messages: null
  };
  await page.route("**/api/v0/conversations**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/api/v0/conversations"))
      return route.fulfill({ json: [conversation] });
    if (url.pathname.endsWith("/messages")) {
      return route.fulfill({
        json: [
          ["In", "hey, are you still sharing the warp records stuff?"],
          ["Out", "yes, browse away"],
          ["In", "thanks!"]
        ].map(([direction, message], n) => ({
          localId: n + 1,
          id: n + 1,
          timestamp: at(10 + n),
          username: "alice",
          direction,
          message,
          isAcknowledged: true,
          wasReplayed: false
        }))
      });
    }
    return route.fulfill({ json: conversation });
  });
};

test("a room reads like a chat: names, actions, links and the wall", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/room.png` });
  // the breadcrumb names the room
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("#indie");
  // bob's two lines are one group under one name, coloured the same everywhere
  await expect(log.getByRole("link", { name: "bob", exact: true })).toHaveCount(1);
  const colour = await log
    .getByRole("link", { name: "bob", exact: true })
    .evaluate((a) => getComputedStyle(a).color);
  const listed = page.getByRole("grid", { name: "Users in the room" });
  await expect(listed.getByRole("link", { name: "bob", exact: true })).toHaveCSS("color", colour);
  // a /me message is an action, a slsk:// link opens Browse, a mention is marked
  await expect(log.getByText("waves at everyone")).toBeVisible();
  await expect(log.getByRole("link", { name: /^slsk:\/\/bob/ })).toHaveAttribute(
    "href",
    /#\/browse\?user=bob&path=Music/
  );
  await expect(log.locator('[data-message="6"]')).toHaveClass(/border-warning/);

  // the member list scrolls inside its panel
  const box = await listed.boundingBox();
  const panel = await page.getByRole("complementary", { name: "Room users" }).boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(panel!.y + panel!.height + 1);

  // the wall lists the tickers
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
  await page.keyboard.press("Escape");
});

/** One event stream frame as the daemon writes it */
const frame = (id: number, event: string, data: unknown) =>
  `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`;

test("a busy room's comings and goings stay out of its messages", async ({ page }) => {
  await fakeChat(page);
  // the first stream that carries the room's frames is a burst of 120 members joining and
  // leaving; every later one is the daemon's own
  let served = false;
  await page.route("**/api/v0/live**", (route) => {
    const topics = new URL(route.request().url()).searchParams.get("topics") ?? "";
    if (served || !topics.split(",").includes("rooms")) return route.continue();
    served = true;
    const hello = {
      principal: { name: "reseek", role: "Administrator" },
      resumed: false,
      serverTime: at(30),
      topics: topics.split(","),
      version: { current: "0.0.0", full: "0.0.0", latest: null, isUpdateAvailable: false }
    };
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
      body: frame(1, "hello", hello) + churn.join("")
    });
  });
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  await page.getByRole("tab", { name: "Activity" }).click();
  const activity = page.getByRole("grid", { name: "Joins, leaves and status changes" });
  await expect(activity.getByRole("row").first()).toContainText("left the room");
  await expect(activity.getByRole("row")).toHaveCount(120);
  // the message list holds the six messages and nothing else
  await expect(log.locator("[data-message]")).toHaveCount(6);
  await expect(log).not.toContainText("joined");
  await expect(log).not.toContainText("left the room");
});

test("a private chat shows both sides with their actions", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/chat");
  await page.goto("./#/chat/alice");
  const log = page.getByRole("log");
  await expect(log.getByText("yes, browse away")).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText("alice");
  // hovering a message offers to copy it
  await log.getByText("thanks!").hover();
  await expect(page.getByRole("button", { name: "Copy the text" }).last()).toBeVisible();
});
