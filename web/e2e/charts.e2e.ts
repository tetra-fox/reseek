import { expect, type Locator, type Page, test } from "@playwright/test";

import { fakeReports } from "./support/fakes";
import { visit } from "./support/login";

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

/**
 * Waits for a chart's marks to grow in. They tween in script, not as animations of the page, so
 * this waits for two frames running to draw the same shapes
 */
const grown = (chart: Locator) =>
  chart.evaluate(
    (box) =>
      new Promise<void>((done) => {
        const shapes = () =>
          [...box.querySelectorAll("path, rect")]
            .map((mark) => ["d", "x", "y", "width", "height"].map((a) => mark.getAttribute(a)))
            .join();
        let last = shapes();
        const frame = () => {
          const now = shapes();
          if (now === last) return done();
          last = now;
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      })
  );

/** Each choice of the history chart and how every label on its value axis reads */
const VIEWS: [string, RegExp][] = [
  ["Size", /^[\d.]+ (B|KB|MB|GB)$/],
  ["Count", /^[\d,]+$/],
  ["Speed", /^[\d.]+ (B|KB|MB)\/s$/],
  ["Error rate", /^[\d.]+%$/],
  ["Queue wait", /^[\d.]+(s|m)$/],
  ["Share ratio", /^\d+\.\d\d$/]
];

/** The labels of a chart's value axis: those at the left edge of its plot, top to bottom */
const valueAxis = (page: Page) =>
  page.getByRole("img", { name: "Transfers over time" }).evaluate((chart) => {
    const labels = [...chart.querySelectorAll("text")].filter((t) => t.textContent?.trim());
    const left = Math.min(...labels.map((t) => t.getBoundingClientRect().left));
    return labels
      .filter(
        (t) => t.getBoundingClientRect().left < left + 60 && t.getAttribute("text-anchor") === "end"
      )
      .map((t) => t.textContent!.trim());
  });

for (const [width, height] of [
  [390, 844],
  [1280, 800]
]) {
  test(`at ${width} px, each view has an axis in its own unit`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await fakeReports(page);
    await visit(page, "/dashboard");
    // the outcomes' labels stay inside their box too
    const outcomes = page.getByRole("img", { name: "Transfer outcomes" }).first();
    await outcomes.scrollIntoViewIfNeeded();
    await expect(outcomes.locator("text").first()).toBeVisible();
    expect(await spills(page, "Transfer outcomes")).toEqual([]);
    const measure = page.getByRole("radiogroup", { name: "What the chart measures" });
    for (const [view, label] of VIEWS) {
      await measure.getByRole("radio", { name: view }).click();
      await expect(measure.getByRole("radio", { name: view })).toBeChecked();
      await expect
        .poll(async () => {
          const labels = await valueAxis(page);
          return labels.length > 1 && labels.every((l) => label.test(l));
        }, view)
        .toBe(true);
      expect(await spills(page, "Transfers over time"), view).toEqual([]);
      const box = await measure.boundingBox();
      expect(box!.x + box!.width, view).toBeLessThanOrEqual(width);
    }
    // a view with one line has nothing to toggle
    await expect(
      page.getByRole("group", { name: "Transfers over time series" }).getByRole("button")
    ).toHaveCount(0);
  });
}

test("a chart cuts its marks at its box while the window narrows", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await fakeReports(page);
  await visit(page, "/dashboard");
  for (const name of ["Transfers over time", "Transfer outcomes"]) {
    const chart = page.getByRole("img", { name }).first();
    await chart.scrollIntoViewIfNeeded();
    await expect(chart.locator("svg").first()).toBeVisible();
    await grown(chart);
    // the furthest the svg, which cuts what it draws, reaches past the box on any frame
    const sampled = chart.evaluate((box) => {
      const svg = box.querySelector("svg")!;
      const end = performance.now() + 500;
      let furthest = -Infinity;
      return new Promise<{ furthest: number; overflow: string }>((done) => {
        const frame = () => {
          const past = svg.getBoundingClientRect().right - box.getBoundingClientRect().right;
          furthest = Math.max(furthest, past);
          if (performance.now() < end) requestAnimationFrame(frame);
          else done({ furthest, overflow: getComputedStyle(svg).overflow });
        };
        frame();
      });
    });
    await page.setViewportSize({ width: 900, height: 800 });
    const { furthest, overflow } = await sampled;
    expect(overflow, name).toBe("hidden");
    expect(furthest, name).toBeLessThan(1);
    await page.setViewportSize({ width: 1280, height: 800 });
  }
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
    await visit(page, "/dashboard");
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
