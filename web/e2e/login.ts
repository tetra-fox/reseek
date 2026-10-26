import { expect, type Page } from "@playwright/test";

/** Logs in with the daemon's built-in account from the login page `route` was sent to */
export const login = async (page: Page, route = "/dashboard") => {
  await page.goto(`./#${route}`);
  await page.getByLabel("Username").fill("reseek");
  await page.getByLabel("Password").fill("reseek");
  await page.getByRole("button", { name: "Login" }).click();
  await expect(page).toHaveURL(new RegExp(`#${route}$`));
};
