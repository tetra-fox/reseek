import { type CDPSession, expect, type Page, test } from "@playwright/test";

import { conversation, fakeChat, fakeConnected, joinedRoom } from "./support/fakes";
import { visit } from "./support/login";

/**
 * The chat fakes with four rooms and `people` conversations, and no stream from the daemon whose
 * empty lists would replace them. The pins themselves are the daemon's
 */
const fakeChats = async (page: Page, people: string[]) => {
  await fakeConnected(page);
  await fakeChat(page);
  const rooms = ["indie", "jazz", "ambient", "techno"].map((name, n) =>
    joinedRoom(name, { lastMessageAt: new Date(Date.UTC(2026, 9, 27, 18, n)).toISOString() })
  );
  await page.route("**/api/v0/rooms/joined", (route) => route.fulfill({ json: rooms }));
  await page.route("**/api/v0/conversations", (route) =>
    route.fulfill({ json: people.map((username) => conversation(username)) })
  );
};

const lines = (page: Page) =>
  page.getByRole("grid", { name: "Rooms and conversations" }).locator("[data-line]");
const line = (page: Page, key: string) => page.locator(`[data-line="${key}"]`);

/** The keys of the rows in Pinned, top first */
const pinned = async (page: Page) => {
  const keys = await lines(page).evaluateAll((all) => all.map((l) => l.getAttribute("data-line")));
  return keys.slice(1, keys.indexOf("h:rooms"));
};

/** Sets the daemon's pins as a page of this site would */
const setPins = (page: Page, data: { kind: string; name: string }[]) =>
  page.request.put("api/v0/chat/pins", { data, headers: { origin: new URL(page.url()).origin } });

/** The daemon's pins, without their revision */
const daemonPins = async (page: Page) =>
  (await (await page.request.get("api/v0/chat/pins")).json()).pins;

const pin = async (page: Page, key: string, name: string) => {
  await line(page, key).hover();
  await line(page, key)
    .getByRole("button", { name: `Menu for ${name}` })
    .click();
  await page.getByRole("menuitem", { name: "Pin", exact: true }).click();
  await expect(line(page, key)).toHaveAttribute("aria-keyshortcuts", /Alt/);
  // a closing menu keeps the page from the pointer for a moment
  await expect(page.locator("body")).toHaveCSS("pointer-events", "auto");
};

const center = async (page: Page, key: string) => {
  const box = (await line(page, key).boundingBox())!;
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
};

test.afterEach(async ({ page }) => {
  await setPins(page, []);
});

test("pins hold their place through a drag, the keys and a reload", async ({ page }) => {
  await fakeChats(page, ["alice", "bob"]);
  await visit(page, "/chat");
  await expect(line(page, "room:jazz")).toBeVisible();
  await expect(line(page, "h:pinned")).toHaveCount(0);

  await pin(page, "room:jazz", "jazz");
  await pin(page, "user:alice", "alice");
  await pin(page, "room:ambient", "ambient");
  await expect.poll(() => pinned(page)).toEqual(["room:jazz", "user:alice", "room:ambient"]);
  // each is in Pinned only, and the sections keep the others in their own order
  await expect(line(page, "room:jazz")).toHaveCount(1);
  await expect(lines(page).first()).toHaveAttribute("data-line", "h:pinned");
  await expect
    .poll(() => lines(page).evaluateAll((all) => all.map((l) => l.getAttribute("data-line"))))
    .toEqual([
      "h:pinned",
      "room:jazz",
      "user:alice",
      "room:ambient",
      "h:rooms",
      "room:techno",
      "room:indie",
      "h:conversations",
      "user:bob",
      "h:notices",
      "user:server"
    ]);

  // the mouse drags the whole row, the others make room, and the drop opens nothing
  const from = await center(page, "room:ambient");
  const to = await center(page, "room:jazz");
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x, from.y - 10, { steps: 3 });
  await page.mouse.move(to.x, to.y - 4, { steps: 8 });
  await expect(line(page, "room:ambient")).toHaveClass(/shadow-raised/);
  await expect
    .poll(() => line(page, "room:jazz").evaluate((row) => getComputedStyle(row).translate))
    .toMatch(/^0px [1-9]/);
  await page.mouse.up();
  await expect.poll(() => pinned(page)).toEqual(["room:ambient", "room:jazz", "user:alice"]);
  await expect(page).toHaveURL(/#\/chat$/);
  await expect(page.getByRole("status").filter({ hasText: "ambient moved" })).toHaveText(
    "ambient moved to place 1 of 3 in Pinned"
  );

  // Escape puts a row back, and a press that does not move is a click
  await page.mouse.move(from.x, to.y);
  const third = await center(page, "user:alice");
  await page.mouse.down();
  await page.mouse.move(third.x, third.y, { steps: 6 });
  await page.keyboard.press("Escape");
  await page.mouse.up();
  await expect.poll(() => pinned(page)).toEqual(["room:ambient", "room:jazz", "user:alice"]);
  await expect(page).toHaveURL(/#\/chat$/);

  // the row stays focused for the keys
  await line(page, "room:jazz").click();
  await expect(page).toHaveURL(/#\/chat\/rooms\/jazz$/);
  await page.getByRole("grid", { name: "Rooms and conversations" }).focus();
  await page.keyboard.press("Alt+Shift+ArrowUp");
  await expect.poll(() => pinned(page)).toEqual(["room:jazz", "room:ambient", "user:alice"]);
  await expect(page).toHaveURL(/#\/chat\/rooms\/jazz$/);
  await expect(page.getByRole("status").filter({ hasText: "jazz moved" })).toHaveText(
    "jazz moved to place 1 of 3 in Pinned"
  );

  await expect
    .poll(() => daemonPins(page))
    .toEqual([
      { kind: "room", name: "jazz" },
      { kind: "room", name: "ambient" },
      { kind: "user", name: "alice" }
    ]);
  await page.reload();
  await expect.poll(() => pinned(page)).toEqual(["room:jazz", "room:ambient", "user:alice"]);

  // Unpin takes a chat back to its section, and a new pin goes last
  await line(page, "room:ambient").hover();
  await line(page, "room:ambient").getByRole("button", { name: "Menu for ambient" }).click();
  await page.getByRole("menuitem", { name: "Unpin" }).click();
  await expect.poll(() => pinned(page)).toEqual(["room:jazz", "user:alice"]);
  await expect(line(page, "room:ambient")).toHaveCount(1);
  await expect(page.locator("body")).toHaveCSS("pointer-events", "auto");
  await pin(page, "room:ambient", "ambient");
  await expect.poll(() => pinned(page)).toEqual(["room:jazz", "user:alice", "room:ambient"]);
  // the daemon holds the last write before the reset after the test
  await expect
    .poll(() => daemonPins(page))
    .toEqual([
      { kind: "room", name: "jazz" },
      { kind: "user", name: "alice" },
      { kind: "room", name: "ambient" }
    ]);
});

test.describe("on a phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });

  type Point = { x: number; y: number };

  /** A finger from `from` to `to` in small moves */
  const swipe = async (client: CDPSession, from: Point, to: Point) => {
    const touch = (type: "touchStart" | "touchMove" | "touchEnd", touchPoints: Point[]) =>
      client.send("Input.dispatchTouchEvent", { type, touchPoints });
    await touch("touchStart", [from]);
    for (let step = 1; step <= 10; step += 1) {
      await touch("touchMove", [{ x: to.x, y: from.y + ((to.y - from.y) * step) / 10 }]);
    }
    await touch("touchEnd", []);
  };

  test("a finger drags a pin by its grip, and scrolls the list anywhere else", async ({ page }) => {
    const people = Array.from({ length: 30 }, (_, n) => `person${n}`);
    await fakeChats(page, people);
    await visit(page, "/chat");
    await setPins(page, [
      { kind: "room", name: "jazz" },
      { kind: "room", name: "indie" },
      { kind: "user", name: "person1" }
    ]);
    await page.reload();
    await expect.poll(() => pinned(page)).toEqual(["room:jazz", "room:indie", "user:person1"]);
    const client = await page.context().newCDPSession(page);
    const box = page.getByRole("grid", { name: "Rooms and conversations" });

    const grip = (await line(page, "room:jazz").locator("button[aria-hidden=true]").boundingBox())!;
    const start = { x: grip.x + grip.width / 2, y: grip.y + grip.height / 2 };
    const end = await center(page, "user:person1");
    await swipe(client, start, { x: start.x, y: end.y + 6 });
    await expect.poll(() => pinned(page)).toEqual(["room:indie", "user:person1", "room:jazz"]);
    await expect(page).toHaveURL(/#\/chat$/);
    await expect(box).toHaveJSProperty("scrollTop", 0);

    const row = await center(page, "room:indie");
    await swipe(client, row, { x: row.x, y: row.y - 200 });
    await expect.poll(() => box.evaluate((e) => e.scrollTop)).toBeGreaterThan(50);
    await expect.poll(() => pinned(page)).toEqual(["room:indie", "user:person1", "room:jazz"]);
    await expect
      .poll(() => daemonPins(page))
      .toEqual([
        { kind: "room", name: "indie" },
        { kind: "user", name: "person1" },
        { kind: "room", name: "jazz" }
      ]);
  });
});
