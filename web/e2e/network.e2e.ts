import { expect, test, type Page } from "@playwright/test";

import { login } from "./login";

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

/**
 * Searches for the peer's files the way a user finds them before downloading; the peer answers
 * by connecting to the daemon
 */
const search = async (page: Page) => {
  const origin = new URL(page.url()).origin;
  const answer = await page.request.post("api/v0/searches", {
    data: { searchText: "album flac", searchTimeout: 3000 },
    headers: { Origin: origin }
  });
  expect(answer.ok(), await answer.text()).toBe(true);
  const id = (await answer.json()).id;
  await expect
    .poll(
      async () => (await (await page.request.get(`api/v0/searches/${id}`)).json()).responseCount,
      {
        timeout: 30_000
      }
    )
    .toBeGreaterThan(0);
};

/** Whether the daemon is logged in to the soulfind serve.sh started, which it is not without one */
const connected = async (page: Page) => {
  const answer = await page.request.get("api/v0/application");
  return answer.ok() && (await answer.json()).server.isLoggedIn === true;
};

test("keeps a selection across two transfers while real ones are added and finish", async ({
  page
}) => {
  await login(page, "/downloads");
  test.skip(!(await connected(page)), "no soulfind and soulseek-rs: the daemon is not connected");

  const table = page.getByRole("grid", { name: "Downloads" });
  // a transfer's own row, not the header of the folder it is grouped under
  const ROW = "[role=row][data-id]:not(:has([role=rowheader]))";
  const rows = table.locator(ROW);
  const succeeded = rows.filter({ hasText: "Succeeded" });
  // the peer can take a moment to come online after the daemon
  await expect(async () => search(page)).toPass({ timeout: 30_000 });
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
