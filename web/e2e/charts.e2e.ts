import { expect, type Page, test } from "@playwright/test";

import { fakeReports } from "./support/fakes";
import { login } from "./support/login";

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

/** The alpha of a computed color, as `rgba(r, g, b, a)` or `oklch(l c h / a)` writes it */
const alpha = (color: string) => {
  const slash = color.match(/\/\s*([\d.]+)(%?)\s*\)$/);
  if (slash) return Number(slash[1]) / (slash[2] ? 100 : 1);
  const rgba = color.match(/^rgba\(.*,\s*([\d.]+)\)$/);
  return rgba ? Number(rgba[1]) : 1;
};

for (const scheme of ["light", "dark"] as const) {
  test(`a chart's tooltip is opaque, ${scheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await fakeReports(page);
    await login(page, "/dashboard");
    for (const name of ["Transfers over time", "Transfer outcomes"]) {
      const chart = page.getByRole("img", { name }).first();
      await chart.scrollIntoViewIfNeeded();
      const box = (await chart.boundingBox())!;
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      const tip = page.locator(".lc-tooltip-container").first();
      await expect(tip, name).toBeVisible();
      const fill = await tip.evaluate((e) => getComputedStyle(e).backgroundColor);
      expect(alpha(fill), `${name}: ${fill}`).toBeGreaterThan(0.97);
    }
  });
}
