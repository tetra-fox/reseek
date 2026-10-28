import { expect, type Page, test } from "@playwright/test";

import { fakeReports } from "./fakes";
import { login } from "./login";

/** The labels of a chart that spill out of its box, its axes' included, by text and how far */
const spills = (page: Page, name: string) =>
  page
    .getByRole("img", { name })
    .first()
    .evaluate((chart) => {
      const box = chart.getBoundingClientRect();
      return [...chart.querySelectorAll("text")].flatMap((label) => {
        const r = label.getBoundingClientRect();
        if (!r.width) return [];
        const out = Math.max(
          box.left - r.left,
          r.right - box.right,
          box.top - r.top,
          r.bottom - box.bottom
        );
        return out > 0.5 ? [`${label.textContent} by ${Math.round(out)}px`] : [];
      });
    });

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  for (const scheme of ["light", "dark"] as const) {
    test.describe(`at ${width} px, ${scheme}`, () => {
      test.use({ viewport: { width, height }, colorScheme: scheme });

      test("the charts' labels stay inside their boxes", async ({ page }) => {
        await fakeReports(page);
        await login(page, "/dashboard");
        for (const name of ["Transfers over time", "Transfer outcomes"]) {
          const chart = page.getByRole("img", { name }).first();
          await chart.scrollIntoViewIfNeeded();
          await expect(chart.locator("text").first()).toBeVisible();
          expect(await spills(page, name), name).toEqual([]);
          if (process.env.SHOTS) {
            await chart.locator("..").screenshot({
              path: `${process.env.SHOTS}/chart-${width}-${scheme}-${name.split(" ")[1]}.png`
            });
          }
        }
      });
    });
  }
}
