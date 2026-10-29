import { expect, type Page, test } from "@playwright/test";

import { WIDEST } from "../src/lib/format";
import {
  fakeAnalysis,
  fakeBrowse,
  fakeChat,
  fakeDownloads,
  fakeFiles,
  fakeFolder,
  fakeReports,
  fakeSearch,
  SEARCH_ID
} from "./fakes";
import { login, textSize } from "./login";

// a phone's width: no screen may be wider than it, so nothing runs off its right edge
test.use({ viewport: { width: 390, height: 844 } });

/**
 * Waits for `ready`, then checks the content area holds the screen: no sideways scroll, and
 * nothing cut off at its right edge by a box that hides what overflows it. A list that scrolls
 * sideways on its own is fine
 */
const holds = async (page: Page, what: string, ready: (page: Page) => Promise<void>) => {
  await ready(page);
  // the window itself never scrolls or zooms out: the content area is what scrolls
  const view = await page.evaluate(() => ({
    width: innerWidth,
    tall: document.documentElement.scrollHeight - document.documentElement.clientHeight,
    wide: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    scrolled: scrollY
  }));
  const width = page.viewportSize()!.width;
  expect(view, `${what} moves the window`).toEqual({ width, tall: 0, wide: 0, scrolled: 0 });
  const [scroll, client] = await page
    .locator("[data-content]")
    .evaluate((e) => [e.scrollWidth, e.clientWidth]);
  expect(scroll, `${what} is ${scroll - client}px wider than its screen`).toBeLessThanOrEqual(
    client
  );
  const cut = await page.locator("[data-content]").evaluate((area) => {
    const edge = area.getBoundingClientRect().right + 0.5;
    const scrolls = (e: Element) => ["auto", "scroll"].includes(getComputedStyle(e).overflowX);
    for (const e of area.querySelectorAll("main *")) {
      const box = e.getBoundingClientRect();
      if (!box.width || box.right <= edge || getComputedStyle(e).visibility === "hidden") continue;
      let inside = false;
      for (let a = e.parentElement; a && a !== area; a = a.parentElement) {
        if (scrolls(a)) inside = true;
      }
      if (!inside) return `${e.tagName.toLowerCase()} "${e.textContent?.trim().slice(0, 40)}"`;
    }
    return null;
  });
  expect(cut, `${what} cuts off ${cut}`).toBeNull();
  // the status bar under the content area holds all it shows within the screen, on more lines if
  // it must, and never scrolls sideways
  const bar = await page.evaluate(() => {
    const footer = [...document.querySelectorAll("footer")].find((f) => !f.closest("main"))!;
    const { left, right } = footer.getBoundingClientRect();
    if (footer.scrollWidth > footer.clientWidth)
      return `is ${footer.scrollWidth - footer.clientWidth}px too wide`;
    for (const e of footer.querySelectorAll("*")) {
      const box = e.getBoundingClientRect();
      if (box.width && (box.left < left - 0.5 || box.right > right + 0.5)) {
        return `cuts off ${e.tagName.toLowerCase()} "${e.textContent?.trim().slice(0, 40)}"`;
      }
    }
    return right > innerWidth + 0.5 ? "runs off the screen" : null;
  });
  expect(bar, `${what}: the status bar ${bar}`).toBeNull();
  // a figure is never cut or painted over its neighbor, a line of a list fits the height it is
  // placed at, a header's label stays in its cell, and a name keeps room for a few letters
  const cells = await page.locator("[data-content]").evaluate((area) => {
    const shown = (e: Element) => e.getClientRects().length > 0;
    for (const e of area.querySelectorAll<HTMLElement>(".tabular-nums")) {
      const style = getComputedStyle(e);
      const kept = style.whiteSpace === "nowrap" || style.textOverflow === "ellipsis";
      if (shown(e) && kept && e.scrollWidth > e.clientWidth + 1) {
        return `the figure "${e.textContent?.trim()}" needs ${e.scrollWidth}px of ${e.clientWidth}px`;
      }
    }
    for (const e of area.querySelectorAll<HTMLElement>("[role=row], [role=treeitem]")) {
      if (shown(e) && e.style.height && e.scrollHeight > e.clientHeight + 1) {
        return `the line "${e.textContent?.trim().slice(0, 40)}" is taller than its place`;
      }
    }
    for (const e of area.querySelectorAll<HTMLElement>("th, [role=columnheader]")) {
      const spills = getComputedStyle(e).overflowX === "visible";
      if (shown(e) && spills && e.scrollWidth > e.clientWidth + 1) {
        return `the header "${e.textContent?.trim()}" spills out of its cell`;
      }
    }
    for (const e of area.querySelectorAll<HTMLElement>(
      "[role=row] .truncate, [role=treeitem] .truncate, tbody tr .truncate"
    )) {
      // three letters' worth, or the whole of a shorter name
      const room = Math.min(3 * parseFloat(getComputedStyle(e).fontSize), e.scrollWidth);
      if (shown(e) && e.textContent?.trim() && e.getBoundingClientRect().width < room - 1) {
        return `the name "${e.textContent.trim()}" has no room`;
      }
    }
    return null;
  });
  expect(cells, `${what}: ${cells}`).toBeNull();
};

/**
 * Checks a drawer at the side holds what it shows: it stays on the screen and nothing in it runs
 * past its right edge, but in a box that scrolls sideways on its own
 */
const drawerHolds = async (page: Page, name: string) => {
  const drawer = page.getByRole("complementary", { name });
  await expect(drawer.getByText(/This is a tool to help you judge, not proof/)).toBeVisible();
  const cut = await drawer.evaluate((aside) => {
    const { left, right } = aside.getBoundingClientRect();
    if (left < -0.5 || right > innerWidth + 0.5) return "the drawer runs off the screen";
    const scrolls = (e: Element) => ["auto", "scroll"].includes(getComputedStyle(e).overflowX);
    for (const e of aside.querySelectorAll("*")) {
      const box = e.getBoundingClientRect();
      if (!box.width || box.right <= right + 0.5) continue;
      let inside = false;
      for (let a = e.parentElement; a && a !== aside; a = a.parentElement) {
        if (scrolls(a)) inside = true;
      }
      if (!inside) return `${e.tagName.toLowerCase()} "${e.textContent?.trim().slice(0, 40)}"`;
    }
    return null;
  });
  expect(cut, `the ${name} drawer cuts off ${cut}`).toBeNull();
};

/** Opens `path` and checks it fits */
const fits = async (page: Page, path: string, ready: (page: Page) => Promise<void>) => {
  await page.goto(`./#${path}`);
  await holds(page, path, ready);
};

const shows = (text: string | RegExp) => async (page: Page) =>
  expect(page.locator("main").getByText(text).first()).toBeVisible();

/** Opens every screen in turn, with the data that fills each, and checks each fits */
const everyScreen = async (page: Page) => {
  await fakeReports(page);
  await fakeSearch(page);
  await fakeChat(page);
  await fakeBrowse(page);
  await fakeFolder(page);
  await fakeFiles(page);
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/dashboard");

  await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
  for (const tab of ["Folders", "Errors"]) {
    await page.getByRole("button", { name: tab, exact: true }).click();
    await holds(
      page,
      `the dashboard's ${tab} tab`,
      shows(tab === "Folders" ? "Most uploaded folders" : "Recent errors")
    );
  }
  await fits(page, "/searches", shows("No searches yet"));
  await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
  // the whole folder's dialog keeps its buttons on the screen
  await page.getByRole("button", { name: "Show whole folder" }).first().click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("04 A Long Track Name.flac")).toBeVisible();
  for (const button of await dialog.getByRole("button").all()) {
    const box = (await button.boundingBox())!;
    expect(box.x, "a dialog button starts off the screen").toBeGreaterThanOrEqual(0);
    expect(box.x + box.width, "a dialog button ends off the screen").toBeLessThanOrEqual(390);
  }
  await page.keyboard.press("Escape");
  await page.getByRole("radio", { name: "Dense" }).click();
  await holds(page, "the dense results", shows("02 Two.flac"));
  await page.getByRole("radio", { name: "Cards" }).click();
  await fits(page, "/downloads", shows("3.flac"));
  // a phone shows no Columns menu, and sorts by any column from the Sort menu
  await expect(page.getByRole("button", { name: "Columns" })).toHaveCount(0);
  await page.getByRole("button", { name: /^Sort:/ }).click();
  await page.getByRole("menuitemradio", { name: "Speed" }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Sort: Speed" })).toBeVisible();
  // a finished download's analysis, with its reading open, fits the drawer
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  await page.getByText("The analyzer's reading", { exact: true }).click();
  await page.getByRole("radio", { name: "Mel" }).click();
  await expect(page.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");
  await drawerHolds(page, "Transfer details");
  await page.keyboard.press("Escape");
  await fits(page, "/uploads", shows("No uploads yet"));
  await fits(page, "/chat", shows("indie"));
  await fits(page, "/chat/rooms/indie", shows("anyone have the new boards of canada rip?"));
  await fits(page, "/chat/alice", shows("yes, browse away"));
  await fits(page, "/users", shows("No users yet"));
  await fits(page, "/users?tab=blocked", shows("No blocked users"));
  await fits(page, "/users/alice", shows("Not seen before"));
  await fits(page, "/browse", shows("Browse a user's shares"));
  await page.getByLabel("Username").fill("carol");
  await page.getByLabel("Username").press("Enter");
  await page.getByRole("treeitem", { name: /Bootlegs/ }).click();
  await holds(page, "a browse with a folder open", shows("2.flac"));
  await fits(page, "/profile", shows("How others see you"));
  for (const [tab, text] of [
    ["info", "Process ID"],
    ["settings", "Listen port"],
    ["account", "API tokens"],
    ["shares", "No shares configured"],
    ["files", "Kid A (Collector's Edition)"],
    ["data", "Finished transfers"],
    ["events", /types/],
    ["logs", "Live"]
  ] as const) {
    await fits(page, `/system/${tab}`, shows(text));
  }

  // past the end of a long screen, the wheel moves nothing else
  await fits(page, "/system/settings", shows("Listen port"));
  await page.locator("[data-content]").evaluate((e) => e.scrollTo({ top: e.scrollHeight }));
  for (let i = 0; i < 6; i++) await page.mouse.wheel(0, 400);
  await holds(page, "settings scrolled past the end", shows("Listen port"));

  // the activity log's button, its only opener on a phone, is on the screen
  const button = (await page.getByRole("button", { name: "Activity" }).boundingBox())!;
  expect(button.x + button.width).toBeLessThanOrEqual(390);
};

test("every screen fits a phone's width", async ({ page }) => {
  await everyScreen(page);
});

test.describe("on a wide screen", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the dashboard and the dense results hold their figures", async ({ page }) => {
    await fakeReports(page);
    await fakeSearch(page);
    await login(page, "/dashboard");
    await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
    await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
    await page.getByRole("radio", { name: "Dense" }).click();
    await holds(page, "the dense results", shows("02 Two.flac"));
    await page.getByRole("radio", { name: "Cards" }).click();
  });

  test("the widest figures fit their width in ch", async ({ page }) => {
    // a tabular digit is wider than 1ch, the plain zero; the narrower point, space and
    // separators of each formatter's widest value make up for it
    await login(page, "/dashboard");
    const over = await page.evaluate((widest) => {
      const span = (text: string, width?: string) => {
        const el = document.createElement("span");
        el.className = "text-sm whitespace-pre tabular-nums";
        el.textContent = text;
        if (width) Object.assign(el.style, { display: "inline-block", width });
        document.body.append(el);
        const w = el.getBoundingClientRect().width;
        el.remove();
        return w;
      };
      return widest
        .map((text) => ({ text, need: span(text), room: span("", `${text.length}ch`) }))
        .filter((f) => f.need > f.room);
    }, Object.values(WIDEST));
    expect(over).toEqual([]);
  });
});

test.describe("at a large text size", () => {
  test.describe("on a wide screen", () => {
    test.use({ viewport: { width: 1280, height: 800 } });

    for (const root of [20, 24]) {
      test(`the figure columns grow with the text at ${root} px`, async ({ page }) => {
        await textSize(page, root);
        await fakeReports(page);
        await fakeSearch(page);
        await fakeDownloads(page);
        await fakeAnalysis(page);
        await login(page, "/dashboard");
        await fits(page, "/dashboard", shows("a_rather_long_username_from_somewhere"));
        await fits(page, `/searches/${SEARCH_ID}`, shows("01 One.flac"));
        await page.getByRole("radio", { name: "Dense" }).click();
        await holds(page, `the dense results at ${root} px`, shows("02 Two.flac"));
        await page.getByRole("radio", { name: "Cards" }).click();
        await fits(page, "/downloads", shows("3.flac"));
        await page.getByRole("button", { name: "More for 2.flac" }).click();
        await page.getByRole("menuitem", { name: "Analyze" }).click();
        await page.getByText("The analyzer's reading", { exact: true }).click();
        await drawerHolds(page, "Transfer details");
        await page.keyboard.press("Escape");
        await fits(page, "/system/info", shows("Process ID"));
      });
    }
  });

  for (const root of [20, 24]) {
    test(`every screen fits a phone at ${root} px`, async ({ page }) => {
      await textSize(page, root);
      await everyScreen(page);
      // the activity log's sheet stands on the status bar, however many lines that has
      const bar = page.locator("footer:not(main footer)");
      const toggle = bar.getByRole("button", { name: "Activity" });
      await toggle.click();
      // polled, as the sheet slides up as it opens
      const gap = async () => {
        const sheet = (await page
          .getByRole("region", { name: "Activity on the network" })
          .boundingBox())!;
        return Math.round(sheet.y + sheet.height - (await bar.boundingBox())!.y);
      };
      await expect.poll(gap, { message: "the sheet's foot is off the bar" }).toBe(0);
    });
  }
});

test("a toast's countdown runs along its foot and holds under the pointer", async ({ page }) => {
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
  await log.getByText("thanks bob, grabbing it now").hover();
  await page.getByRole("button", { name: "Copy the text" }).last().click();
  const toast = page.getByRole("alert").filter({ hasText: "Couldn't copy" });
  await expect(toast).toBeVisible();
  const bar = toast.locator(".countdown");
  // measured once it has flown in, as the two boxes are read one after the other
  await toast.evaluate((e) => Promise.all(e.getAnimations().map((a) => a.finished)));
  const [box, line] = [(await toast.boundingBox())!, (await bar.boundingBox())!];
  expect(box.x + box.width, "the toast fits the phone").toBeLessThanOrEqual(390);
  expect(line.height).toBeLessThanOrEqual(3);
  expect(line.x).toBeGreaterThanOrEqual(box.x);
  expect(line.x + line.width).toBeLessThanOrEqual(box.x + box.width);
  expect(Math.abs(line.y + line.height - (box.y + box.height))).toBeLessThanOrEqual(1);
  const countdown = () =>
    bar.evaluate((e) => {
      const [animation] = e.getAnimations();
      return { state: animation.playState, at: Number(animation.currentTime) };
    });
  await toast.hover();
  await expect.poll(async () => (await countdown()).state).toBe("paused");
  const held = await countdown();
  await page.waitForTimeout(300);
  expect((await countdown()).at, "the bar holds while the pointer is on the toast").toBe(held.at);
  await page.mouse.move(0, 0);
  await expect.poll(async () => (await countdown()).state).toBe("running");
});
