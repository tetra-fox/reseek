import { expect, type Locator, type Page, test } from "@playwright/test";

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

/** What a chart's marks are on every frame for `ms`: how far past its box, and their paths */
const marks = (chart: Locator, ms: number) =>
  chart.evaluate((box, ms) => {
    const seen: { past: number; paths: string }[] = [];
    const end = performance.now() + ms;
    return new Promise<typeof seen>((done) => {
      const frame = () => {
        const shapes = [...box.querySelectorAll("path, rect")];
        const right = box.getBoundingClientRect().right;
        seen.push({
          past: Math.max(...shapes.map((s) => s.getBoundingClientRect().right - right)),
          paths: shapes.map((s) => s.getAttribute("d")).join()
        });
        if (performance.now() < end) requestAnimationFrame(frame);
        else done(seen);
      };
      frame();
    });
  }, ms);

test("a chart's marks stay in its box while the window narrows", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await fakeReports(page);
  await login(page, "/dashboard");
  for (const name of ["Transfers over time", "Transfer outcomes"]) {
    const chart = page.getByRole("img", { name }).first();
    await chart.scrollIntoViewIfNeeded();
    await expect(chart.locator("path, rect").first()).toBeVisible();
    await page.waitForTimeout(700); // the marks have grown in
    const sampled = marks(chart, 500);
    await page.setViewportSize({ width: 900, height: 800 });
    expect(Math.max(...(await sampled).map((f) => f.past)), name).toBeLessThan(1);
    await page.setViewportSize({ width: 1280, height: 800 });
  }
});

test("a series turned on still grows in", async ({ page }) => {
  await fakeReports(page);
  await login(page, "/dashboard");
  const chart = page.getByRole("img", { name: "Transfers over time" }).first();
  await chart.scrollIntoViewIfNeeded();
  await expect(chart.locator("path").first()).toBeVisible();
  await page.waitForTimeout(700); // the marks have grown in
  const toggle = page
    .getByRole("group", { name: "Transfers over time series" })
    .getByRole("button")
    .first();
  await toggle.click();
  const sampled = marks(chart, 400);
  await toggle.click();
  expect(new Set((await sampled).map((f) => f.paths)).size).toBeGreaterThan(3);
});

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
