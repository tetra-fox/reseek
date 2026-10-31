import { expect, test, type Page } from "@playwright/test";

import { login } from "./support/login";

/** The peer serve.sh starts beside the daemon, and the files it shares */
const PEER = "smokepeer";
const file = (n: number) => ({ filename: `share\\Album\\${n}.flac`, size: 200_000 });

/** Asks the daemon, through the page's session, to download `files` from the peer */
const enqueue = async (page: Page, files: { filename: string; size: number }[]) => {
  const origin = new URL(page.url()).origin;
  const answer = await page.request.post(`api/v0/transfers/downloads/${PEER}`, {
    data: files,
    headers: { Origin: origin }
  });
  expect(answer.status(), await answer.text()).toBe(201);
};

/** Whether the server reports the peer online */
const peerOnline = async (page: Page) => {
  const answer = await page.request.get(`api/v0/users/${PEER}/status`);
  return answer.ok() && (await answer.json()).presence === "Online";
};

/** Whether the daemon is logged in to the soulfind serve.sh started, which it is not without one */
const connected = async (page: Page) => {
  const answer = await page.request.get("api/v0/application");
  return answer.ok() && (await answer.json()).server.isLoggedIn === true;
};

test("the server's answer to adding a member shows once, and stays under Server", async ({
  page
}) => {
  await login(page, "/chat");
  test.skip(!(await connected(page)), "no soulfind and soulseek-rs: the daemon is not connected");
  await expect.poll(() => peerOnline(page), { timeout: 30_000 }).toBe(true);
  const room = "smoke_private";
  const created = await page.request.post("api/v0/rooms/joined", {
    data: { name: room, private: true },
    headers: { Origin: new URL(page.url()).origin }
  });
  expect(created.status(), await created.text()).toBeLessThan(300);

  await page.goto(`./#/chat/rooms/${room}`);
  await page.getByRole("textbox", { name: "Username to add" }).fill(PEER);
  await page.getByRole("button", { name: "Add", exact: true }).click();
  const answer = page.getByText(/^Soulseek server: /);
  await expect(answer).toHaveCount(1);
  await expect(page.getByText("New message from server")).toHaveCount(0);

  const entry = page.getByRole("row", { name: /^Server/ });
  await expect(entry).toBeVisible();
  await page.reload();
  await entry.click();
  await expect(page).toHaveURL(/#\/chat\/server$/);
  await expect(page.getByRole("log").locator("[data-message]")).not.toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Message" })).toHaveCount(0);
  await expect(page.getByRole("row", { name: /^server/ })).toHaveCount(0);

  await page.getByRole("button", { name: "Server notices menu" }).click();
  await page.getByRole("menuitem", { name: "Clear history" }).click();
  await page.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(page).toHaveURL(/#\/chat$/);
  await expect(entry).toHaveCount(0);
});

test("looks up a user the server has, and says so of one it does not", async ({ page }) => {
  await login(page, "/users");
  test.skip(!(await connected(page)), "no soulfind and soulseek-rs: the daemon is not connected");
  await expect.poll(() => peerOnline(page), { timeout: 30_000 }).toBe(true);
  const box = page.getByRole("combobox", { name: "Search users" });

  await box.fill("nobody-has-this-name");
  await page.getByRole("option", { name: "Look up nobody-has-this-name" }).click();
  await expect(page.getByText("No user named nobody-has-this-name was found.")).toBeVisible();
  await expect(page).toHaveURL(/#\/users$/);

  await box.fill(PEER);
  await page.getByRole("option", { name: `Look up ${PEER}` }).click();
  await expect(page).toHaveURL(new RegExp(`#/users/${PEER}$`));
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toContainText(PEER);
  // what the network said is written a moment after it is shown
  const stored = async () =>
    (await (await page.request.get(`api/v0/users/${PEER}/profile`)).json()).known;
  await expect.poll(stored).toBe(true);
  await page.goto("./#/users");
  await expect(page.locator(`[data-line="${PEER}"]`)).toBeVisible();
  await expect(page.locator('[data-line="nobody-has-this-name"]')).toHaveCount(0);
});

test("keeps a selection across two transfers while real ones are added and finish", async ({
  page
}) => {
  await login(page, "/downloads");
  test.skip(!(await connected(page)), "no soulfind and soulseek-rs: the daemon is not connected");

  const table = page.getByRole("treegrid", { name: "Downloads" });
  // a transfer's own row, not the header of the folder it is grouped under
  const ROW = "[role=rowgroup] [role=row][aria-selected]";
  const rows = table.locator(ROW);
  const succeeded = rows.filter({ hasText: "Succeeded" });
  // the peer can take a moment to come online after the daemon
  await expect.poll(() => peerOnline(page), { timeout: 30_000 }).toBe(true);
  await enqueue(page, [file(1), file(2)]);
  await expect(rows).toHaveCount(2);
  await expect(succeeded).toHaveCount(2, { timeout: 30_000 });

  // text selected from the first row into the second, the way a user drags across them
  const selected = await rows.first().evaluate((first, row) => {
    const second = first.parentElement!.querySelectorAll(row)[1];
    document.getSelection()!.setBaseAndExtent(first, 0, second, second.childNodes.length);
    Object.assign(window, { held: { first, second } });
    return document.getSelection()!.toString();
  }, ROW);
  expect(selected).not.toBe("");

  // more downloads arrive by push and run to the end while the selection is held
  await enqueue(page, [file(3), file(4)]);
  await expect(rows).toHaveCount(4);
  await expect(succeeded).toHaveCount(4, { timeout: 30_000 });

  const kept = await page.evaluate(() => {
    const { first, second } = (window as unknown as { held: { first: Node; second: Node } }).held;
    const selection = document.getSelection()!;
    return {
      text: selection.toString(),
      sameNodes:
        first.isConnected &&
        second.isConnected &&
        selection.anchorNode === first &&
        selection.focusNode === second
    };
  });
  expect(kept.sameNodes).toBe(true);
  expect(kept.text).toBe(selected);
});
