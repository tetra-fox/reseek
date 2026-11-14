import { expect, type Locator, type Page } from "@playwright/test";

import { open } from "./login";

/**
 * The first element under `root` matching `selector` that runs past `root`'s right edge, as a
 * description, unless a box between them scrolls sideways on its own. Run in the page
 */
const cutOff = (root: Element, selector: string) => {
  const edge = root.getBoundingClientRect().right + 0.5;
  const scrolls = (e: Element) => ["auto", "scroll"].includes(getComputedStyle(e).overflowX);
  for (const e of root.querySelectorAll(selector)) {
    const box = e.getBoundingClientRect();
    if (!box.width || box.right <= edge || getComputedStyle(e).visibility === "hidden") continue;
    let inside = false;
    for (let a = e.parentElement; a && a !== root; a = a.parentElement) {
      if (scrolls(a)) inside = true;
    }
    if (!inside) return `${e.tagName.toLowerCase()} "${e.textContent?.trim().slice(0, 40)}"`;
  }
  return null;
};

/**
 * Waits for `ready`, then checks the content area holds the screen: no sideways scroll, and
 * nothing cut off at its right edge by a box that hides what overflows it. A list that scrolls
 * sideways on its own is fine
 */
export const holds = async (page: Page, what: string, ready: (page: Page) => Promise<void>) => {
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
  const cut = await page.locator("[data-content]").evaluate(cutOff, "main *");
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
 * Checks a layer holds what it shows: it stays on the screen and nothing in it runs past its right
 * edge, but in a box that scrolls sideways on its own
 */
export const layerHolds = async (layer: Locator, name: string) => {
  const off = await layer.evaluate((box) => {
    const { left, right } = box.getBoundingClientRect();
    return left < -0.5 || right > innerWidth + 0.5;
  });
  expect(off, `the ${name} runs off the screen`).toBe(false);
  const cut = await layer.evaluate(cutOff, "*");
  expect(cut, `the ${name} cuts off ${cut}`).toBeNull();
};

/** Opens `path` and checks it fits */
export const fits = async (page: Page, path: string, ready: (page: Page) => Promise<void>) => {
  await open(page, `${path}`);
  await holds(page, path, ready);
};

export const shows = (text: string | RegExp) => async (page: Page) =>
  expect(page.locator("main").getByText(text).first()).toBeVisible();
