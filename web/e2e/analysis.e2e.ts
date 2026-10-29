import { expect, type Locator, test } from "@playwright/test";

import { newId } from "../src/lib/ids";
import { ANALYZED_DOWNLOAD, fakeAnalysis, fakeDownloads } from "./fakes";
import { login } from "./login";
import { wav } from "./pictures";

/** A plot's place in the canvas, in CSS pixels, as the canvas writes it */
const rect = async (plot: Locator, which = "data-plot") => {
  const [x, y, width, height] = (await plot.getAttribute(which))!.split(" ").map(Number);
  return { x, y, width, height };
};

/** The color at a point `across` and `down` a plot, from 0 to 1 */
const pixel = async (plot: Locator, across: number, down: number, which = "data-plot") => {
  const r = await rect(plot, which);
  return plot.evaluate(
    (canvas: HTMLCanvasElement, at) => {
      const ratio = canvas.width / canvas.clientWidth;
      const [red, green, blue] = canvas
        .getContext("2d")!
        .getImageData(Math.floor(at.x * ratio), Math.floor(at.y * ratio), 1, 1).data;
      return [red, green, blue];
    },
    { x: r.x + across * r.width, y: r.y + down * r.height }
  );
};

/** Whether the cursor's white line is drawn within two pixels of a point on a plot */
const cursorNear = async (plot: Locator, across: number, down: number, which = "data-plot") => {
  const r = await rect(plot, which);
  for (let dx = -2; dx <= 2; dx++) {
    const color = await pixel(plot, across + dx / r.width, down, which);
    if (color.every((c) => c === 255)) return true;
  }
  return false;
};

/** How far down the plot a guide line at `hz` is drawn, on a 22.05 kHz file's axis */
const guideRow = async (plot: Locator, hz: number, scale: "linear" | "mel" = "linear") => {
  const r = await rect(plot);
  const position = scale === "mel" ? mel(hz) / mel(22_050) : hz / 22_050;
  // the line is a device pixel thick on the row it rounds to; its middle in CSS pixels
  const ratio = await plot.evaluate((c: HTMLCanvasElement) => c.width / c.clientWidth);
  const row = Math.round((1 - position) * (r.height * ratio - 1));
  return (row + 0.5) / ratio / r.height;
};
const mel = (hz: number) => 2595 * Math.log10(1 + hz / 700);

/** A locator's box once it stops moving, as a drawer slides in and scrolls to what it shows */
const settled = async (locator: Locator) => {
  let last = await locator.boundingBox();
  for (;;) {
    await locator.page().waitForTimeout(100);
    const now = await locator.boundingBox();
    if (now && last && now.x === last.x && now.y === last.y && now.width === last.width) {
      return now;
    }
    last = now;
  }
};

/** Clicks a plot `across` it, from 0 to 1, half way down */
const clickAt = async (plot: Locator, across: number) => {
  const r = await rect(plot);
  await plot.click({ position: { x: r.x + across * r.width, y: r.y + r.height / 2 } });
};

test("analyzes a finished download from its row and keeps a judgment", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
  const table = page.getByRole("grid", { name: "Downloads" });
  // the analyzed download says so in its row, muted, with the reading in its tooltip
  await expect(table.getByText("Analyzed", { exact: true })).toHaveCount(1);
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();

  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  await expect(drawer.getByRole("heading", { name: "Analysis" })).toBeInViewport();
  const plot = drawer.getByRole("slider", { name: "Cursor" });
  await expect(plot).toBeVisible();
  await expect(drawer.getByText(/This analysis is an aid to judgment, not proof/)).toBeVisible();
  // each guide line shows and hides from its legend, drawn on the plot at its frequency
  const legend = drawer.getByRole("button", { name: "MP3 128k" });
  const guide = await guideRow(plot, 16_000);
  await expect.poll(() => pixel(plot, 0.5, guide)).toEqual([90, 210, 255]);
  await legend.click();
  await expect(legend).toHaveAttribute("aria-pressed", "false");
  await expect.poll(() => pixel(plot, 0.5, guide)).not.toEqual([90, 210, 255]);
  await legend.click();
  await expect.poll(() => pixel(plot, 0.5, guide)).toEqual([90, 210, 255]);

  // the user's judgment, saved with its note and a tick
  await drawer.getByRole("radio", { name: "Fake" }).click();
  await drawer.getByLabel("Note").fill("sounds like an old rip");
  await drawer.getByRole("button", { name: "Save judgment" }).click();
  await expect(drawer.getByRole("status").filter({ hasText: "Saved" })).toBeVisible();
  await expect(drawer.getByRole("button", { name: "Save judgment" })).toBeDisabled();
  // the row now shows the user's own judgment
  await expect(table.getByText("Fake", { exact: true })).toBeVisible();

  // the analyzer's reading, folded away and labeled as its own
  await drawer.getByText("Analyzer reading").click();
  await expect(drawer.getByText("Likely a transcode")).toBeVisible();

  // the cursor steps a window at a time from the keyboard, ten with Shift
  await plot.focus();
  await page.keyboard.press("ArrowRight");
  await expect(plot).toHaveAttribute("aria-valuenow", "1");
  await expect(drawer.getByText("Spectrum at 0:01")).toBeVisible();
  await expect(
    drawer.getByRole("img", { name: "The spectrum of the window at 0:01" })
  ).toBeVisible();
  await page.keyboard.press("Shift+ArrowRight");
  await expect(plot).toHaveAttribute("aria-valuenow", "11");
  await page.keyboard.press("End");
  await expect(drawer.getByText("Spectrum at 0:29")).toBeVisible();
  await page.keyboard.press("Shift+ArrowLeft");
  await expect(plot).toHaveAttribute("aria-valuenow", "19");
  await page.keyboard.press("Home");
  await expect(plot).toHaveAttribute("aria-valuenow", "0");
  await expect(plot).toHaveAttribute(
    "aria-valuetext",
    /^0:00\.0, in the window at 0:00: -20\.0 dBFS, cutoff at 16\.8 kHz$/
  );

  // a click on the plot puts the cursor at that moment
  await clickAt(plot, 10.5 / 30.5);
  await expect(drawer.getByText("Spectrum at 0:10")).toBeVisible();

  // the frequency scale is the viewer's to choose, kept in this browser
  const scales = drawer.getByRole("radiogroup", { name: "Frequency scale" });
  await expect(scales.getByRole("radio", { name: "Linear" })).toHaveAttribute(
    "aria-checked",
    "true"
  );
  const linear = await guideRow(plot, 16_000);
  await scales.getByRole("radio", { name: "Mel" }).click();
  await expect(scales.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");
  // the guide line moves to where 16 kHz is on a mel axis
  await expect
    .poll(async () => pixel(plot, 0.5, await guideRow(plot, 16_000, "mel")))
    .toEqual([90, 210, 255]);
  expect(await guideRow(plot, 16_000, "mel")).toBeLessThan(linear);
  // the cursor stays where it was
  await expect(drawer.getByText("Spectrum at 0:10")).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  await expect(scales.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");

  // and the whole file can be seen across the screen, the cursor where the drawer has it
  await drawer.getByRole("slider", { name: "Cursor" }).focus();
  await page.keyboard.press("Shift+ArrowRight");
  await drawer.getByRole("button", { name: "Open large" }).click();
  const large = page.getByRole("dialog", { name: "2.flac" });
  const wide = large.getByRole("slider", { name: "Cursor" });
  await expect(wide).toBeVisible();
  const narrow = (await drawer.getByRole("slider", { name: "Cursor" }).boundingBox())!.width;
  expect((await settled(wide)).width).toBeGreaterThan(narrow * 1.5);
  await expect(wide).toHaveAttribute("aria-valuenow", "10");
  await page.keyboard.press("Escape");
  await expect(large).toBeHidden();
});

test("dragging the cursor on the waveform moves it on the spectrogram and the spectrum", async ({
  page
}) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  const plot = drawer.getByRole("slider", { name: "Cursor" });
  const spectrum = drawer
    .getByRole("img", { name: /^The spectrum of the window at/ })
    .locator("path")
    .last();
  await expect(drawer.getByText("Spectrum at 0:00")).toBeVisible();
  const before = await spectrum.getAttribute("d");
  // the cursor sits at the start; grab it on the waveform and drag it two thirds across
  const wave = await rect(plot, "data-wave");
  const box = await settled(plot);
  const y = box.y + wave.y + wave.height / 2;
  await page.mouse.move(box.x + wave.x + 1, y);
  await page.mouse.down();
  for (let i = 1; i <= 10; i++) {
    await page.mouse.move(box.x + wave.x + (wave.width * 2 * i) / 30, y);
  }
  await page.mouse.up();
  await expect(plot).toHaveAttribute("aria-valuenow", /^20(\.\d)?$/);
  await expect(drawer.getByText("Spectrum at 0:20")).toBeVisible();
  expect(await spectrum.getAttribute("d")).not.toBe(before);
  // the line is drawn across the spectrogram above, where the waveform has it
  const at = Number(await plot.getAttribute("aria-valuenow")) / 30.5;
  await expect.poll(() => cursorNear(plot, at, 0.5)).toBe(true);
  await expect.poll(() => cursorNear(plot, at, 0.5, "data-wave")).toBe(true);
});

test("zooms around the pointer, pans, and loads the detail in view", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page, { duration: 300 });
  const tiles = new Set<string>();
  page.on("request", (r) => {
    const tile = new URL(r.url()).searchParams.get("tile");
    if (tile !== null && r.url().includes("/analysis/spectrogram")) tiles.add(tile);
  });
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  const plot = drawer.getByRole("slider", { name: "Cursor" });
  await expect(plot).toHaveAttribute("data-view", "0.000 300.000");
  // the plain wheel scrolls the drawer and says how to zoom; with Ctrl it zooms at the pointer
  const spec = await rect(plot, "data-plot");
  const box = await settled(plot);
  await page.mouse.move(box.x + spec.x + spec.width / 4, box.y + spec.height / 2);
  await page.mouse.wheel(0, 100);
  await expect(drawer.getByText("Hold Ctrl and scroll to zoom.")).toBeVisible();
  await page.keyboard.down("Control");
  for (let i = 0; i < 8; i++) await page.mouse.wheel(0, -200);
  await page.keyboard.up("Control");
  const view = async () => (await plot.getAttribute("data-view"))!.split(" ").map(Number);
  await expect.poll(async () => (await view())[1]).toBeLessThan(30);
  // the time under the pointer stays under it
  const [start, span] = await view();
  // within the pixel the pointer rounds to at the whole file's 300 s
  expect(Math.abs(start + span / 4 - 75)).toBeLessThan(1);
  // the detail for what is in view loads, and nothing far from it
  await expect.poll(() => tiles.size).toBeGreaterThan(0);
  // tiles load once the view is narrower than the overview has columns for, around 100 s
  for (const tile of tiles) expect(Number(tile) * 16).toBeLessThan(200);

  // dragging away from the cursor pans
  const y = box.y + spec.height / 2;
  await page.mouse.move(box.x + spec.x + spec.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(box.x + spec.x + spec.width / 4, y, { steps: 5 });
  await page.mouse.up();
  await expect.poll(async () => (await view())[0]).toBeCloseTo(start + span / 4, 0);
  // the zoom buttons work around the cursor, and the whole file comes back with one press
  await drawer.getByRole("button", { name: "Show the whole file" }).click();
  await expect(plot).toHaveAttribute("data-view", "0.000 300.000");
  await drawer.getByRole("button", { name: "Zoom in" }).click();
  await expect.poll(async () => (await view())[1]).toBe(150);
  // a press on the strip of the whole file centers the view there
  const strip = drawer.locator("canvas[aria-hidden=true]");
  const stripBox = (await strip.boundingBox())!;
  await strip.click({ position: { x: stripBox.width * 0.8, y: stripBox.height / 2 } });
  await expect.poll(async () => (await view())[0]).toBe(150);
});

test("pans a zoomed five-minute file at sixty frames a second", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page, { duration: 300 });
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  await drawer.getByRole("button", { name: "Open large" }).click();
  const plot = page.getByRole("dialog", { name: "2.flac" }).getByRole("slider", { name: "Cursor" });
  const detail = page.waitForResponse((r) => r.url().includes("/analysis/spectrogram?tile="));
  for (let i = 0; i < 4; i++) {
    await page.getByRole("dialog").getByRole("button", { name: "Zoom in" }).click();
  }
  // the detail for the first stretch arrives before the clock starts
  await detail;
  const box = await settled(plot);
  const spec = await rect(plot, "data-plot");
  const y = box.y + spec.height / 2;
  // the main thread's work, which is what decides whether a frame is ready in time; the frame
  // gaps also count this headless browser's software compositing
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable");
  const work = async () => {
    const { metrics } = await cdp.send("Performance.getMetrics");
    const value = (name: string) => metrics.find((m) => m.name === name)!.value * 1000;
    return { task: value("TaskDuration"), script: value("ScriptDuration") };
  };
  const before = await work();
  const frames = page.evaluate(
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
  );
  await page.mouse.move(box.x + spec.x + spec.width * 0.9, y);
  await page.mouse.down();
  for (let i = 0; i < 120; i++) {
    await page.mouse.move(box.x + spec.x + spec.width * (0.9 - (0.8 * i) / 120), y);
  }
  await page.mouse.up();
  const gaps = (await frames).toSorted((a, b) => a - b);
  const after = await work();
  const elapsed = gaps.reduce((a, b) => a + b, 0);
  const mean = elapsed / gaps.length;
  const p95 = gaps[Math.floor(gaps.length * 0.95)];
  const task = (after.task - before.task) / gaps.length;
  const script = (after.script - before.script) / gaps.length;
  // kept with the test's result, where the report shows it
  test.info().annotations.push({
    type: "panning",
    description:
      `frames ${mean.toFixed(1)} ms on average, p95 ${p95.toFixed(1)} ms; main thread ` +
      `${task.toFixed(1)} ms a frame, ${script.toFixed(1)} ms of it script`
  });
  expect(task).toBeLessThan(1000 / 60);
});

/** The rows the MP3 128k guide line is drawn on, sampled every frame until `ms` have passed */
const guideRows = (plot: Locator, ms: number) =>
  plot.evaluate(
    (canvas: HTMLCanvasElement, ms) =>
      new Promise<number[]>((done) => {
        const [x, y, width, height] = canvas.dataset.plot!.split(" ").map(Number);
        const ratio = canvas.width / canvas.clientWidth;
        const context = canvas.getContext("2d")!;
        const rows: number[] = [];
        const start = performance.now();
        const tick = () => {
          const column = context.getImageData(
            Math.floor((x + width / 2) * ratio),
            Math.round(y * ratio),
            1,
            Math.round(height * ratio)
          ).data;
          for (let row = 0; row < column.length / 4; row++) {
            const at = row * 4;
            if (column[at] === 90 && column[at + 1] === 210 && column[at + 2] === 255) {
              rows.push(row);
              break;
            }
          }
          if (performance.now() - start < ms) requestAnimationFrame(tick);
          else done(rows);
        };
        requestAnimationFrame(tick);
      }),
    ms
  );

test("the frequency scale moves between linear and mel through the frames between", async ({
  page
}) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  const plot = drawer.getByRole("slider", { name: "Cursor" });
  const scales = drawer.getByRole("radiogroup", { name: "Frequency scale" });
  await expect(scales.getByRole("radio", { name: "Linear" })).toHaveAttribute(
    "aria-checked",
    "true"
  );
  const row = async (scale: "linear" | "mel") => {
    const { height } = await rect(plot);
    const ratio = await plot.evaluate((c: HTMLCanvasElement) => c.width / c.clientWidth);
    return Math.round((await guideRow(plot, 16_000, scale)) * height * ratio - 0.5);
  };
  const linear = await row("linear");
  const mel = await row("mel");
  await expect.poll(async () => (await guideRows(plot, 0)).at(-1)).toBe(linear);

  const watched = guideRows(plot, 600);
  await scales.getByRole("radio", { name: "Mel" }).click();
  // the choice is saved at the click, while the plot is still on its way
  expect(await page.evaluate(() => localStorage.getItem("analysis.spectrogram"))).toContain("mel");
  const rows = await watched;
  // the line climbs a row at a time or more, never back, through rows of neither scale
  const between = new Set(rows.filter((r) => r < linear && r > mel));
  expect(between.size).toBeGreaterThan(3);
  for (let i = 1; i < rows.length; i++) expect(rows[i]).toBeLessThanOrEqual(rows[i - 1]);
  expect(rows.at(-1)).toBe(mel);
});

test("moves between scales at sixty frames a second on a five-minute file", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page, { duration: 300 });
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  await drawer.getByRole("button", { name: "Open large" }).click();
  const dialog = page.getByRole("dialog", { name: "2.flac" });
  const detail = page.waitForResponse((r) => r.url().includes("/analysis/spectrogram?tile="));
  for (let i = 0; i < 4; i++) await dialog.getByRole("button", { name: "Zoom in" }).click();
  await detail;
  await settled(dialog.getByRole("slider", { name: "Cursor" }));
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable");
  const work = async () => {
    const { metrics } = await cdp.send("Performance.getMetrics");
    const value = (name: string) => metrics.find((m) => m.name === name)!.value * 1000;
    return { task: value("TaskDuration"), script: value("ScriptDuration") };
  };
  const scales = dialog.getByRole("radiogroup", { name: "Frequency scale" });
  const before = await work();
  const frames = page.evaluate(
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
  );
  // a new scale every 200 ms, each before the last has landed, so every frame is a moving one
  let counted = false;
  void frames.then(() => (counted = true));
  for (let i = 0; !counted; i++) {
    await scales.getByRole("radio", { name: ["Log", "Mel", "Linear"][i % 3] }).click();
    await page.waitForTimeout(200);
  }
  const gaps = (await frames).toSorted((a, b) => a - b);
  const after = await work();
  const mean = gaps.reduce((a, b) => a + b, 0) / gaps.length;
  const p95 = gaps[Math.floor(gaps.length * 0.95)];
  const task = (after.task - before.task) / gaps.length;
  const script = (after.script - before.script) / gaps.length;
  test.info().annotations.push({
    type: "scale moves",
    description:
      `frames ${mean.toFixed(1)} ms on average, p95 ${p95.toFixed(1)} ms; main thread ` +
      `${task.toFixed(1)} ms a frame, ${script.toFixed(1)} ms of it script`
  });
  expect(task).toBeLessThan(1000 / 60);
});

test("the cursor follows playback and a click seeks it", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await page.route("**/api/v0/transfers/downloads/alice/*/content**", (route) =>
    route.fulfill({ body: wav(6), contentType: "audio/wav" })
  );
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  const plot = drawer.getByRole("slider", { name: "Cursor" });
  await plot.focus();
  await page.keyboard.press("ArrowRight");
  await drawer.getByRole("button", { name: "Play", exact: true }).click();
  // playback starts at the cursor and the cursor moves with it
  const now = async () => Number(await plot.getAttribute("aria-valuenow"));
  await expect.poll(now).toBeGreaterThan(1.5);
  const audio = page.locator("audio");
  const position = () => audio.evaluate((a: HTMLAudioElement) => a.currentTime);
  // a click puts the cursor there and playback goes on from it
  await clickAt(plot, 4 / 30.5);
  await expect.poll(position).toBeGreaterThanOrEqual(4);
  expect(await position()).toBeLessThan(5);
  await drawer.getByRole("button", { name: "Pause", exact: true }).click();
  const paused = await now();
  await page.waitForTimeout(300);
  expect(await now()).toBe(paused);
});

test("a report an older version made asks to be analyzed again", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page, { stale: true });
  const queued: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST" && r.url().includes("/analysis")) queued.push(r.url());
  });
  await login(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  await expect(
    drawer.getByText(
      "This file was analyzed by an older version of reseek. Analyze it again to view the results."
    )
  ).toBeVisible();
  await expect(drawer.getByRole("slider", { name: "Cursor" })).toHaveCount(0);
  // the judgment is the user's and outlives the report
  await expect(drawer.getByRole("radiogroup", { name: "Your judgment" })).toBeVisible();
  // asked to analyze, it is queued as a file with no report would be
  await expect.poll(() => queued.length).toBe(1);
  await drawer.getByRole("button", { name: "Analyze", exact: true }).click();
  await expect.poll(() => queued.length).toBe(2);
});

test("filters downloads by their analysis, by chip and by word", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
  const table = page.getByRole("grid", { name: "Downloads" });
  const shown = (name: string) => table.getByRole("button", { name: `More for ${name}` });
  await expect(shown("3.flac")).toBeVisible();

  const analysis = page.getByRole("group", { name: "Analysis" });
  await analysis.getByRole("button", { name: /^Analyzed/ }).click();
  await expect(shown("2.flac")).toBeVisible();
  await expect(shown("3.flac")).toHaveCount(0);
  // a finished flac without a report is waiting to be analyzed; the running one is not
  await analysis.getByRole("button", { name: /^Not analyzed/ }).click();
  await expect(shown("3.flac")).toBeVisible();
  await expect(shown("2.flac")).toHaveCount(0);
  await expect(shown("1.flac")).toHaveCount(0);
  await analysis.getByRole("button", { name: /^Not analyzed/ }).click();

  await page.getByRole("searchbox", { name: "Filter" }).fill("analyzed:yes");
  await expect(shown("2.flac")).toBeVisible();
  await expect(shown("3.flac")).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Filter" }).fill("marked:fake");
  await expect(shown("2.flac")).toHaveCount(0);
});

test("queues a user's downloads, a folder and a selection for analysis", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  const queued: string[] = [];
  await page.route(
    (url) => /\/transfers\/downloads\/[^/]+\/[^/]+\/analysis$/.test(url.pathname),
    (route) => {
      if (route.request().method() !== "POST") return route.fallback();
      const id = new URL(route.request().url()).pathname.split("/").at(-2)!;
      if (id === ANALYZED_DOWNLOAD) {
        return route.fulfill({
          status: 409,
          json: { error: "the file has been analyzed already" }
        });
      }
      queued.push(id);
      return route.fulfill({ status: 202, json: { jobId: newId() } });
    }
  );
  await login(page, "/downloads");
  const grid = page.getByRole("grid", { name: "Downloads" });
  const header = (name: string) => grid.getByRole("rowheader").filter({ hasText: name });

  // alice's running download is not finished and her other one is analyzed already
  await header("alice").getByRole("button", { name: "Analyze the group" }).click();
  await expect(
    page.getByText(
      "No files queued for analysis. 2 files skipped: 1 not finished and 1 already analyzed or queued."
    )
  ).toBeVisible();
  expect(queued).toEqual([]);

  await header("Big").getByRole("button", { name: "Analyze folder" }).click();
  await expect(page.getByText("1 file queued for analysis", { exact: true })).toBeVisible();
  expect(queued).toHaveLength(1);

  // a selection of nothing finished offers no Analyze
  await grid.getByRole("checkbox", { name: "Select 1.flac" }).check();
  const bar = page.getByRole("toolbar", { name: "Selected transfers" });
  await expect(bar.getByRole("button", { name: "Cancel" })).toBeVisible();
  await expect(bar.getByRole("button", { name: "Analyze" })).toHaveCount(0);
  await grid.getByRole("checkbox", { name: "Select 3.flac" }).check();
  await bar.getByRole("button", { name: "Analyze" }).click();
  await expect(
    page.getByText("1 file queued for analysis. 1 file skipped: 1 not finished.")
  ).toBeVisible();
  expect(queued).toHaveLength(2);
});

test("analyzes a file and queues a folder from the Files screen", async ({ page }) => {
  await fakeAnalysis(page);
  const at = "2026-10-27T18:00:00Z";
  const file = (fullName: string) => ({
    attributes: "Normal",
    createdAt: at,
    modifiedAt: at,
    fullName,
    name: fullName.split("/").pop(),
    length: 30_000_000
  });
  await page.route("**/api/v0/files/downloads/directories", (route) =>
    route.fulfill({
      json: {
        attributes: "Directory",
        createdAt: at,
        modifiedAt: at,
        fullName: "",
        name: "downloads",
        directories: [
          {
            attributes: "Directory",
            createdAt: at,
            modifiedAt: at,
            fullName: "Album",
            name: "Album",
            directories: null,
            files: null
          }
        ],
        files: [file("track.flac"), file("notes.txt")]
      }
    })
  );
  const queued: { id: string; path: string }[] = [];
  await page.route("**/api/v0/files/downloads/directories/*", (route) =>
    route.fulfill({
      json: {
        attributes: "Directory",
        createdAt: at,
        modifiedAt: at,
        fullName: "",
        name: "Album",
        directories: [],
        files: [file("01.flac"), file("cover.jpg"), file("CD2/02.wav")]
      }
    })
  );
  await page.route(
    (url) => url.pathname.includes("/files/downloads/files/") && url.pathname.endsWith("/analysis"),
    (route) => {
      // the file's path travels base64url-encoded between /files/ and /analysis
      const encoded = new URL(route.request().url()).pathname.split("/files/").at(-1) ?? "";
      const path = Buffer.from(encoded.split("/")[0], "base64url").toString();
      if (route.request().method() === "POST") {
        const id = newId();
        queued.push({ id, path });
        return route.fulfill({ status: 202, json: { jobId: id } });
      }
      // the folder's files have no report yet
      if (path.startsWith("Album/")) {
        return route.fulfill({ status: 404, json: { error: "the file has not been analyzed" } });
      }
      return route.fallback();
    }
  );
  // the queue, with track.flac queued from another browser before this page opened
  const job = (id: string, path: string) => ({
    id,
    path: `/srv/downloads/${path}`,
    root: "downloads",
    relativePath: path,
    transferId: null,
    state: "queued",
    queuedAt: at,
    startedAt: null,
    progress: null
  });
  await page.route("**/api/v0/analysis/jobs", (route) =>
    route.fulfill({
      json: [job(newId(), "track.flac"), ...queued.map((q) => job(q.id, q.path))]
    })
  );
  await login(page, "/system/files");
  await expect(page.getByRole("button", { name: "More for notes.txt" })).toBeVisible();
  await page.getByRole("button", { name: "More for notes.txt" }).click();
  await expect(page.getByRole("menuitem", { name: "Analyze" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  // the analyzed file says so, and the filter by the analyzer's reading finds it
  const files = page.getByRole("treegrid");
  await expect(files.getByText("Analyzed", { exact: true })).toBeVisible();
  const show = page.getByRole("combobox", { name: "Show" });
  await show.click();
  await page.getByRole("option", { name: "Analyzer: likely a transcode" }).click();
  await expect(page.getByRole("button", { name: "More for track.flac" })).toBeVisible();
  await show.click();
  await page.getByRole("option", { name: "Not analyzed" }).click();
  await expect(page.getByRole("button", { name: "More for track.flac" })).toHaveCount(0);
  await show.click();
  await page.getByRole("option", { name: "All files" }).click();

  await page.getByRole("button", { name: "More for track.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  const drawer = page.getByRole("complementary", { name: "Analysis" });
  // queued by another browser, it shows its place in the queue
  await expect(drawer.getByText("Waiting in the analysis queue")).toBeVisible();
  await expect(drawer.getByRole("slider", { name: "Cursor" })).toBeVisible();
  await expect(drawer.getByText(/This analysis is an aid to judgment, not proof/)).toBeVisible();
  // the file's own actions sit beside its name
  await expect(drawer.getByRole("button", { name: "Preview" })).toBeVisible();
  await expect(drawer.getByRole("button", { name: "Download to device" })).toBeVisible();
  await drawer.getByRole("button", { name: "Close the analysis" }).click();

  await page.getByRole("button", { name: "More for Album" }).click();
  await page.getByRole("menuitem", { name: "Analyze folder" }).click();
  await expect(
    page.getByText("2 files queued for analysis. 1 file skipped: 1 not an audio type.")
  ).toBeVisible();
  expect(queued.map((q) => q.path)).toEqual(["Album/01.flac", "Album/CD2/02.wav"]);

  // a file the folder queued shows its place in the queue, not an offer to analyze it
  await page.getByRole("button", { name: "More for Album" }).click();
  await page.getByRole("menuitem", { name: "Open" }).click();
  await page.getByRole("button", { name: "More for 01.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyze" }).click();
  await expect(drawer.getByText("Waiting in the analysis queue")).toBeVisible();
  await expect(drawer.getByRole("button", { name: "Analyze", exact: true })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Analysis queue" })).toBeVisible();
});
