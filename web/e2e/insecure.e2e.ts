import { expect, type Page, test } from "@playwright/test";

import { fakeChat, fakeConnected } from "./support/fakes";
import { login } from "./support/login";

// these run on a hostname over plain http that is not localhost, as a LAN address is, where the
// page has no secure context and the browser leaves out crypto.randomUUID, navigator.clipboard
// and notifications

/** Logs in and checks the page really has no secure context, collecting any uncaught error */
const insecure = async (page: Page, route: string) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await login(page, route);
  expect(await page.evaluate(() => isSecureContext)).toBe(false);
  return errors;
};

test("starts a search", async ({ page }) => {
  // the test daemon never connects to a server, and a search is offered only while it is
  await fakeConnected(page);
  const errors = await insecure(page, "/dashboard");
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByRole("combobox", { name: /Go to a screen/ }).fill("album");
  await page.getByRole("option", { name: "Search for album" }).click();
  await expect(page).toHaveURL(/#\/searches\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-/);
  expect(errors).toEqual([]);
});

test("copies a message by selecting it, and says notifications need HTTPS", async ({ page }) => {
  // what the browser was asked to copy, read from the box the copy selects
  await page.addInitScript(() => {
    const copy = document.execCommand.bind(document);
    document.execCommand = (command: string, ...rest: [boolean?, string?]) => {
      const box = document.activeElement;
      if (command === "copy" && box instanceof HTMLTextAreaElement) {
        (window as unknown as { copied: string }).copied = box.value;
      }
      return copy(command, ...rest);
    };
  });
  await fakeChat(page);
  const errors = await insecure(page, "/chat");
  await page.goto("./#/chat/rooms/indie");
  await expect(page.getByText(/Browsers show desktop notifications only over HTTPS/)).toBeVisible();
  const log = page.getByRole("log");
  await log.getByText("thanks bob, grabbing it now").hover();
  await page.getByRole("button", { name: "Copy text" }).last().click();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { copied?: string }).copied))
    .toBe("thanks bob, grabbing it now");
  await expect(page.getByRole("alert").filter({ hasText: "Could not copy" })).toHaveCount(0);
  expect(errors).toEqual([]);
});
