import { expect, type Page, test } from "@playwright/test";

import { openSlot, rect, settled } from "../support/analysis";
import { fakeAnalysis, fakeDownloads } from "../support/fakes";
import { visit } from "../support/login";

/**
 * Runs `act` while the page draws 120 frames, notes the frames' gaps and the main thread's work
 * with the test's result, where the report shows them, and gives that work per frame. The main
 * thread's work is what decides whether a frame is ready in time; the gaps also count this
 * headless browser's software compositing. `act` is told once the frames are counted
 */
const workPerFrame = async (
  page: Page,
  what: string,
  act: (counted: () => boolean) => Promise<void>
) => {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable");
  const work = async () => {
    const { metrics } = await cdp.send("Performance.getMetrics");
    const value = (name: string) => metrics.find((m) => m.name === name)!.value * 1000;
    return { task: value("TaskDuration"), script: value("ScriptDuration") };
  };
  const before = await work();
  let counted = false;
  const frames = page
    .evaluate(
      () =>
        new Promise<number[]>((done) => {
          const times: number[] = [];
          const tick = (t: number) => {
            times.push(t);
            if (times.length < 121) requestAnimationFrame(tick);
            else done(times.slice(1).map((t, i) => t - times[i]));
          };
          requestAnimationFrame(tick);
        })
    )
    .then((gaps) => {
      counted = true;
      return gaps;
    });
  await act(() => counted);
  const gaps = (await frames).toSorted((a, b) => a - b);
  const after = await work();
  const mean = gaps.reduce((a, b) => a + b, 0) / gaps.length;
  const p95 = gaps[Math.floor(gaps.length * 0.95)];
  const task = (after.task - before.task) / gaps.length;
  const script = (after.script - before.script) / gaps.length;
  test.info().annotations.push({
    type: what,
    description:
      `frames ${mean.toFixed(1)} ms on average, p95 ${p95.toFixed(1)} ms; main thread ` +
      `${task.toFixed(1)} ms a frame, ${script.toFixed(1)} ms of it script`
  });
  return task;
};

/** A five-minute download open in slot A, zoomed in four times, its detail in */
const zoomedSlot = async (page: Page) => {
  await fakeDownloads(page);
  await fakeAnalysis(page, { duration: 300 });
  await visit(page, "/downloads");
  const slot = await openSlot(page);
  const detail = page.waitForResponse((r) => r.url().includes("/analysis/spectrogram?tile="));
  for (let i = 0; i < 4; i++) await slot.getByRole("button", { name: "Zoom in" }).click();
  // the detail for the first stretch arrives before the clock starts
  await detail;
  return slot;
};

test("pans a zoomed five-minute file at sixty frames a second", async ({ page }) => {
  const plot = (await zoomedSlot(page)).getByRole("slider", { name: "Cursor" });
  const box = await settled(plot);
  const spec = await rect(plot, "data-plot");
  const y = box.y + spec.height / 2;
  const task = await workPerFrame(page, "panning", async () => {
    await page.mouse.move(box.x + spec.x + spec.width * 0.9, y);
    await page.mouse.down();
    for (let i = 0; i < 120; i++) {
      await page.mouse.move(box.x + spec.x + spec.width * (0.9 - (0.8 * i) / 120), y);
    }
    await page.mouse.up();
  });
  expect(task).toBeLessThan(1000 / 60);
});

test("moves between scales at sixty frames a second on a five-minute file", async ({ page }) => {
  const slot = await zoomedSlot(page);
  await settled(slot.getByRole("slider", { name: "Cursor" }));
  const scales = slot.getByRole("radiogroup", { name: "Frequency scale" });
  const task = await workPerFrame(page, "scale moves", async (counted) => {
    // a new scale as soon as the last is taken, well before its move lands, so every frame is a
    // moving one
    for (let i = 0; !counted(); i++) {
      const scale = scales.getByRole("radio", { name: ["Log", "Mel", "Linear"][i % 3] });
      await scale.click();
      await expect(scale).toHaveAttribute("aria-checked", "true");
    }
  });
  expect(task).toBeLessThan(1000 / 60);
});
