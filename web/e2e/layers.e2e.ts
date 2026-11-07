import { expect, test } from "@playwright/test";

import { fakeChat } from "./support/fakes";
import { visit } from "./support/login";

for (const [width, height] of [
  [1280, 800],
  [390, 844]
]) {
  test.describe(`at ${width} px`, () => {
    test.use({ viewport: { width, height } });

    test("the chat fills the content area, edge to edge", async ({ page }) => {
      await fakeChat(page);
      await visit(page, "/chat/rooms/indie");
      await expect(page.getByText("anyone have the new boards of canada rip?")).toBeVisible();
      const gutter = await page.evaluate(() => {
        const area = document.querySelector("[data-content]")!.getBoundingClientRect();
        const chat = document.querySelector("[data-chat]")!.getBoundingClientRect();
        return [
          chat.left - area.left,
          area.right - chat.right,
          chat.top - area.top,
          area.bottom - chat.bottom
        ].map(Math.round);
      });
      expect(gutter, "the gutter left, right, above and below").toEqual([0, 0, 0, 0]);
    });
  });
}
