import { expect, test } from "@playwright/test";

import { fakeChat, frame, helloFrame } from "./fakes";
import { login } from "./login";

test("a room reads like a chat: names, actions, links and the wall", async ({ page }) => {
  await fakeChat(page);
  await login(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  const log = page.getByRole("log");
  await expect(log.getByText("anyone have the new boards of canada rip?")).toBeVisible();
  if (process.env.SHOTS) await page.screenshot({ path: `${process.env.SHOTS}/room.png` });
  // the breadcrumb names the room
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
  // a /me message is an action, a slsk:// link opens Browse, a mention is marked
  await expect(log.getByText("waves at everyone")).toBeVisible();
  // a message's text is for reading and copying: the I-beam; the name above it is a link
  await expect(log.getByText("320 too if you want it smaller")).toHaveCSS("cursor", "text");
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
  // the card's More opens the name's menu, which no button beside the name offers
  await page
    .getByRole("dialog", { name: "Profile of alice" })
    .getByRole("button", { name: "More" })
    .click();
  await expect(page.getByRole("menuitem", { name: "Copy username" })).toBeVisible();
  await page.keyboard.press("Escape");
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

test("a toast sits above the docked activity log", async ({ page }) => {
  // a copy the browser refuses both ways is a toast that needs nothing from the daemon
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: () => Promise.reject(new Error("refused")) }
    });
    document.execCommand = () => false;
  });
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
  const toast = page.getByRole("alert").filter({ hasText: "Couldn't copy" });
  await expect(toast).toBeVisible();
  const [above, top] = [(await toast.boundingBox())!, (await dock.boundingBox())!];
  expect(above.y + above.height).toBeLessThanOrEqual(top.y);
});
