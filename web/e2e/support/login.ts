import { expect, type Page } from "@playwright/test";

/**
 * Opens `route` with the session the login project saved, once the app is up on it. A goto in a
 * running app changes only the hash and returns, and the hash router answers with a full reload:
 * this waits for that page, and for its main area, shown once its router has started
 */
export const open = async (page: Page, route: string) => {
  const loaded = page.waitForEvent("load");
  await page.goto(`./#${route}`);
  await loaded;
  await expect(page.getByRole("main")).toBeVisible();
};

/** Logs in through the form with the daemon's built-in account, from the login page `route` was sent to */
export const login = async (page: Page, route = "/dashboard") => {
  await page.goto(`./#${route}`);
  await page.getByLabel("Username").fill("reseek");
  await page.getByLabel("Password").fill("reseek");
  await page.getByRole("button", { name: "Log in", exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`#${route}$`));
};

/** Opens `route` as `open` does, and checks the app stays on it rather than going elsewhere */
export const visit = async (page: Page, route: string) => {
  await open(page, route);
  await expect(page).toHaveURL(new RegExp(`#${route.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`));
};

/**
 * A browser's larger font sizes: at a root of 20 px, "Large", the app's 14 px text is 17.5 px.
 * Set before every page, since a navigation may load a new one
 */
export const textSize = (page: Page, root: number) =>
  page.addInitScript((px) => {
    document.addEventListener("DOMContentLoaded", () => {
      document.documentElement.style.fontSize = `${px}px`;
    });
  }, root);

/** A clipboard the browser refuses both ways, so a copy is a toast that needs nothing from the daemon */
export const refuseCopy = (page: Page) =>
  page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: () => Promise.reject(new Error("refused")) }
    });
    document.execCommand = () => false;
  });
