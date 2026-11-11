import { expect, type Locator, type Page, test } from "@playwright/test";

import type { Schemas } from "../src/lib/api/client";
import { newId } from "../src/lib/ids";
import { openSlot, rect, settled } from "./support/analysis";
import {
  ANALYZED_DOWNLOAD,
  fakeAnalysis,
  fakeBatch,
  fakeDownloads,
  listedFile,
  listedFolder
} from "./support/fakes";
import { open, visit } from "./support/login";
import { wav } from "./support/pictures";

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

/** How far across a plot guide lines are read: the exact middle of a stroke can come out blended */
const GUIDE_AT = 0.4;

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

/** Opens the inspector on a download from its row */
const details = async (page: Page) => {
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Details" }).click();
  return page.getByRole("complementary", { name: "Details" });
};

/** Clicks a plot `across` it, from 0 to 1, half way down */
const clickAt = async (plot: Locator, across: number) => {
  const r = await rect(plot);
  await plot.click({ position: { x: r.x + across * r.width, y: r.y + r.height / 2 } });
};

test("analyzes a finished download from its row and keeps a judgment", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await visit(page, "/downloads");
  const table = page.getByRole("treegrid", { name: "Downloads" });
  // the analyzed download says so in its row, muted, with the reading in its tooltip
  await expect(table.getByText("Analyzed", { exact: true })).toHaveCount(1);
  const inspector = await details(page);
  // the inspector gives the verdict in a line, with the spectrogram small
  await expect(inspector.getByText("Likely a transcode").first()).toBeVisible();
  await expect(inspector.getByText("A guide, not proof")).toBeVisible();
  await inspector.getByRole("button", { name: "Open in Analysis" }).click();

  const viewer = page.getByRole("region", { name: "Slot A" });
  const plot = viewer.getByRole("slider", { name: "Cursor" });
  await expect(plot).toBeVisible();
  // the guide opens from the toolbar with the disclaimer
  await page.getByRole("button", { name: "Guide" }).click();
  await expect(page.getByText(/The analysis is a guide, not proof/)).toBeVisible();
  await page.keyboard.press("Escape");
  // each guide line shows and hides from its legend, drawn on the plot at its frequency
  const legend = viewer.getByRole("button", { name: "MP3 128k" });
  const guide = await guideRow(plot, 16_000);
  await expect.poll(() => pixel(plot, GUIDE_AT, guide)).toEqual([90, 210, 255]);
  await legend.click();
  await expect(legend).toHaveAttribute("aria-pressed", "false");
  await expect.poll(() => pixel(plot, GUIDE_AT, guide)).not.toEqual([90, 210, 255]);
  await legend.click();
  await expect.poll(() => pixel(plot, GUIDE_AT, guide)).toEqual([90, 210, 255]);

  // the user's judgment, saved with its note and a tick
  await viewer.getByRole("radio", { name: "Fake" }).click();
  await viewer.getByLabel("Note").fill("sounds like an old rip");
  await viewer.getByRole("button", { name: "Save judgment" }).click();
  await expect(viewer.getByRole("status").filter({ hasText: "Saved" })).toBeVisible();
  await expect(viewer.getByRole("button", { name: "Save judgment" })).toBeDisabled();

  // the analyzer's reading, folded away and labeled as its own
  await viewer.getByText("Analyzer reading").click();
  await expect(viewer.getByText("Likely a transcode")).toBeVisible();

  // the cursor steps a window at a time from the keyboard, ten with Shift
  await plot.focus();
  await page.keyboard.press("ArrowRight");
  await expect(plot).toHaveAttribute("aria-valuenow", "1");
  await expect(viewer.getByText("Spectrum at 0:01")).toBeVisible();
  await expect(viewer.getByRole("img", { name: "Spectrum at 0:01" })).toBeVisible();
  await page.keyboard.press("Shift+ArrowRight");
  await expect(plot).toHaveAttribute("aria-valuenow", "11");
  await page.keyboard.press("End");
  await expect(viewer.getByText("Spectrum at 0:29")).toBeVisible();
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
  await expect(viewer.getByText("Spectrum at 0:10")).toBeVisible();

  // the frequency scale is the viewer's to choose, kept in this browser
  const scales = viewer.getByRole("radiogroup", { name: "Frequency scale" });
  await expect(scales.getByRole("radio", { name: "Linear" })).toHaveAttribute(
    "aria-checked",
    "true"
  );
  const linear = await guideRow(plot, 16_000);
  await scales.getByRole("radio", { name: "Mel" }).click();
  await expect(scales.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");
  // the guide line moves to where 16 kHz is on a mel axis
  await expect
    .poll(async () => pixel(plot, GUIDE_AT, await guideRow(plot, 16_000, "mel")))
    .toEqual([90, 210, 255]);
  expect(await guideRow(plot, 16_000, "mel")).toBeLessThan(linear);
  // the cursor stays where it was
  await expect(viewer.getByText("Spectrum at 0:10")).toBeVisible();
  // the address brings the file back after a reload, on the scale chosen
  await page.reload();
  await expect(scales.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");

  // the file's line in the recent results, which an empty slot A shows, has the user's judgment
  await viewer.getByRole("button", { name: "Clear slot A" }).click();
  const recent = page.getByRole("region", { name: "Recent results" });
  await expect(recent.getByText("Fake", { exact: true })).toBeVisible();

  // and the list's row shows the judgment
  await open(page, "/downloads");
  await expect(table.getByText("Fake", { exact: true })).toBeVisible();
});

test("dragging the cursor on the waveform moves it on the spectrogram and the spectrum", async ({
  page
}) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await visit(page, "/downloads");
  const viewer = await openSlot(page);
  const plot = viewer.getByRole("slider", { name: "Cursor" });
  const spectrum = viewer
    .getByRole("img", { name: /^Spectrum at/ })
    .locator("path")
    .last();
  await expect(viewer.getByText("Spectrum at 0:00")).toBeVisible();
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
  await expect(viewer.getByText("Spectrum at 0:20")).toBeVisible();
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
  await visit(page, "/downloads");
  const viewer = await openSlot(page);
  const plot = viewer.getByRole("slider", { name: "Cursor" });
  await expect(plot).toHaveAttribute("data-view", "0.000 300.000");
  // the wheel zooms at the pointer
  const spec = await rect(plot, "data-plot");
  const box = await settled(plot);
  await page.mouse.move(box.x + spec.x + spec.width / 4, box.y + spec.height / 2);
  for (let i = 0; i < 8; i++) await page.mouse.wheel(0, -200);
  const view = async () => (await plot.getAttribute("data-view"))!.split(" ").map(Number);
  /** The view once two reads 100 ms apart agree, as a zoom eases in */
  const settledView = async () => {
    // the poll's first try is at once, so the first view is read there and compared 100 ms on
    let last: Awaited<ReturnType<typeof view>> | undefined;
    await expect
      .poll(
        async () => {
          const now = await view();
          const still = !!last && now[0] === last[0] && now[1] === last[1];
          last = now;
          return still;
        },
        { message: "the view keeps moving", intervals: [100] }
      )
      .toBe(true);
    return last!;
  };
  await expect.poll(async () => (await view())[1]).toBeLessThan(30);
  // the time under the pointer stays under it
  const [start, span] = await settledView();
  // within the pixel the pointer rounds to at the whole file's 300 s
  expect(Math.abs(start + span / 4 - 75)).toBeLessThan(1);
  // the detail for what is in view loads
  for (let tile = Math.floor(start / 16); tile * 16 < start + span; tile++) {
    await expect.poll(() => tiles.has(String(tile))).toBe(true);
  }

  // dragging away from the cursor pans
  const y = box.y + spec.height / 2;
  await page.mouse.move(box.x + spec.x + spec.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(box.x + spec.x + spec.width / 4, y, { steps: 5 });
  await page.mouse.up();
  await expect.poll(async () => (await view())[0]).toBeCloseTo(start + span / 4, 0);

  // a jump across the file loads the detail where it lands, and nothing far from it. Measured
  // from a settled view: a zoom's first frames show most of the file and ask for tiles across it
  const strip = viewer.locator("canvas[aria-hidden=true]");
  const stripBox = (await strip.boundingBox())!;
  tiles.clear();
  await strip.click({ position: { x: stripBox.width * 0.8, y: stripBox.height / 2 } });
  await expect.poll(() => tiles.size).toBeGreaterThan(0);
  const [landed, shown] = await settledView();
  for (const tile of tiles) {
    expect(Number(tile) * 16 + 16).toBeGreaterThan(landed - shown / 2);
    expect(Number(tile) * 16).toBeLessThan(landed + shown * 1.5);
  }

  // the zoom buttons work around the cursor, and the whole file comes back with one press
  await viewer.getByRole("button", { name: "Show whole file" }).click();
  await expect(plot).toHaveAttribute("data-view", "0.000 300.000");
  await viewer.getByRole("button", { name: "Zoom in" }).click();
  await expect.poll(async () => (await view())[1]).toBe(150);
  // a press on the strip of the whole file centers the view there
  await strip.click({ position: { x: stripBox.width * 0.8, y: stripBox.height / 2 } });
  await expect.poll(async () => (await view())[0]).toBe(150);
});

test("the cursor follows playback and a click seeks it", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  // the bytes of the range the browser asks for, as the daemon serves them: a whole file answered
  // to a range leaves it unseekable, and a seek starts it over
  const sound = wav(6);
  await page.route("**/api/v0/transfers/downloads/alice/*/content**", (route) => {
    const [first, last] = (route.request().headers()["range"] ?? "bytes=0-")
      .replace("bytes=", "")
      .split("-")
      .map((n, i) => (n ? Number(n) : i ? sound.length - 1 : 0));
    return route.fulfill({
      status: 206,
      body: sound.subarray(first, last + 1),
      headers: {
        "content-type": "audio/wav",
        "accept-ranges": "bytes",
        "content-range": `bytes ${first}-${last}/${sound.length}`
      }
    });
  });
  await visit(page, "/downloads");
  const viewer = await openSlot(page);
  const plot = viewer.getByRole("slider", { name: "Cursor" });
  await plot.focus();
  await page.keyboard.press("ArrowRight");
  await viewer.getByRole("button", { name: "Play", exact: true }).click();
  // playback starts at the cursor and the cursor moves with it
  const now = async () => Number(await plot.getAttribute("aria-valuenow"));
  await expect.poll(now).toBeGreaterThan(1.5);
  const audio = page.locator("audio");
  const position = () => audio.evaluate((a: HTMLAudioElement) => a.currentTime);
  await viewer.getByRole("button", { name: "Pause", exact: true }).click();
  await expect(audio).toHaveJSProperty("paused", true);
  // a click puts the cursor there and the sound with it, to the pixel the click lands on
  await clickAt(plot, 4 / 30.5);
  await expect.poll(position).toBeGreaterThan(3.9);
  expect(await position()).toBeLessThan(4.1);
  // and playback goes on from there
  await viewer.getByRole("button", { name: "Play", exact: true }).click();
  await expect(audio).toHaveJSProperty("paused", false);
  expect(await position()).toBeGreaterThan(3.9);
  await expect.poll(now).toBeGreaterThan(4.2);
  // the sound belongs to the slot and goes with it
  await viewer.getByRole("button", { name: "Clear slot A" }).click();
  await expect(viewer.getByText("Add a file to analyze")).toBeVisible();
  await expect(audio).toHaveCount(0);
});

test("a report an older version made asks to be analyzed again", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page, { stale: true });
  const queued: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST" && r.url().includes("/analysis")) queued.push(r.url());
  });
  await visit(page, "/downloads");
  const slot = await openSlot(page);
  await expect(
    slot.getByText("Results from an older version of reseek can't be shown")
  ).toBeVisible();
  // it offers, and does not start by itself
  expect(queued).toHaveLength(0);
  await slot.getByRole("button", { name: "Analyze again" }).click();
  await expect.poll(() => queued.length).toBe(1);
  // the judgment is the user's and outlives the report; there is no spectrogram to show with it
  await expect(slot.getByRole("radiogroup", { name: "Your judgment" })).toBeVisible();
  await expect(slot.getByRole("slider", { name: "Cursor" })).toHaveCount(0);
});

test("filters downloads by their analysis, by chip and by word", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await visit(page, "/downloads");
  const table = page.getByRole("treegrid", { name: "Downloads" });
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
  await fakeBatch(page, (file) => {
    if (file.kind !== "download") return "refused";
    if (file.id === ANALYZED_DOWNLOAD) return "current";
    queued.push(file.id);
    return "queued";
  });
  await visit(page, "/downloads");
  const grid = page.getByRole("treegrid", { name: "Downloads" });
  const header = (name: string) => grid.getByRole("rowheader").filter({ hasText: name });

  // alice's running download is not finished and her other one is analyzed already
  await header("alice").getByRole("button", { name: "Analyze" }).click();
  await expect(
    page.getByText(
      "No files queued for analysis. 2 files skipped: 1 not finished and 1 already analyzed or queued."
    )
  ).toBeVisible();
  expect(queued).toEqual([]);

  await header("Big").getByRole("button", { name: "Analyze" }).click();
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
  await page.route("**/api/v0/files/downloads/directories", (route) =>
    route.fulfill({
      json: listedFolder("", "downloads", {
        directories: [listedFolder("Album", "Album")],
        files: [listedFile("track.flac"), listedFile("notes.txt")]
      })
    })
  );
  const queued: { id: string; path: string }[] = [];
  await fakeBatch(page, (file) => {
    if (file.kind !== "file") return "refused";
    queued.push({ id: newId(), path: file.path });
    return "queued";
  });
  await page.route("**/api/v0/files/downloads/directories/*", (route) =>
    route.fulfill({
      json: listedFolder("", "Album", {
        directories: [],
        files: [listedFile("01.flac"), listedFile("cover.jpg"), listedFile("CD2/02.wav")]
      })
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
  const job = (id: string, path: string) =>
    ({
      id,
      path: `/srv/downloads/${path}`,
      root: "downloads",
      relativePath: path,
      transferId: null,
      state: "queued",
      queuedAt: "2026-10-27T18:00:00Z",
      startedAt: null,
      progress: null
    }) satisfies Schemas["AnalysisJob"];
  await page.route("**/api/v0/analysis/jobs", (route) =>
    route.fulfill({
      json: {
        jobs: [job(newId(), "track.flac"), ...queued.map((q) => job(q.id, q.path))],
        backlog: 0
      }
    })
  );
  await visit(page, "/system/files");
  await expect(page.getByRole("button", { name: "More for notes.txt" })).toBeVisible();
  await page.getByRole("button", { name: "More for notes.txt" }).click();
  await expect(page.getByRole("menuitem", { name: "Open in Analysis" })).toHaveCount(0);
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
  await page.getByRole("menuitem", { name: "Details" }).click();
  const inspector = page.getByRole("complementary", { name: "Details" });
  // queued by another browser, it shows its place in the queue
  await expect(inspector.getByText("Waiting in queue")).toBeVisible();
  await expect(inspector.getByText("Likely a transcode").first()).toBeVisible();
  // the file's own actions sit under its name, the rest in its menu
  await expect(inspector.getByRole("button", { name: "Open", exact: true })).toBeVisible();
  await inspector.getByRole("button", { name: "More actions" }).click();
  await expect(page.getByRole("menuitem", { name: "Save to this device" })).toBeVisible();
  await page.keyboard.press("Escape");
  await inspector.getByRole("button", { name: "Close details" }).click();

  await page.getByRole("button", { name: "More for Album" }).click();
  await page.getByRole("menuitem", { name: "Analyze", exact: true }).click();
  await expect(
    page.getByText("2 files queued for analysis. 1 file skipped: 1 not an audio type.")
  ).toBeVisible();
  expect(queued.map((q) => q.path)).toEqual(["Album/01.flac", "Album/CD2/02.wav"]);

  // a file the folder queued shows its place in the queue, not an offer to analyze it
  await page.getByRole("button", { name: "More for Album" }).click();
  await page.getByRole("menuitem", { name: "Open" }).click();
  await page.getByRole("button", { name: "More for 01.flac" }).click();
  await page.getByRole("menuitem", { name: "Details" }).click();
  await expect(inspector.getByText("Waiting in queue")).toBeVisible();
  await inspector.getByRole("button", { name: "Open in Analysis" }).click();
  // the toolbar counts the queue and opens it
  await page.getByRole("button", { name: /^Queue/ }).click();
  await expect(page.getByRole("heading", { name: "Analysis queue" })).toBeVisible();
});

test("opens a file, compares a second, and clears them, with the address following", async ({
  page
}) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await visit(page, "/downloads");
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  // with nothing to compare to, a list offers only to open
  await expect(page.getByRole("menuitem", { name: "Compare" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  const a = await openSlot(page);
  const b = page.getByRole("region", { name: "Slot B" });
  await expect(a.getByRole("slider", { name: "Cursor" })).toBeVisible();
  await expect(b.getByRole("button", { name: "Add from Downloads" })).toBeVisible();
  await expect(page).toHaveURL(/#\/analysis\?a=download/);
  const compare = page.getByRole("button", { name: "Compare" });
  await expect(compare).toBeDisabled();

  // Compare is there now, and fills the second slot
  await page.getByRole("link", { name: "Downloads" }).click();
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Compare" }).click();
  await expect(b.getByRole("slider", { name: "Cursor" })).toBeVisible();
  await expect(page).toHaveURL(/#\/analysis\?a=download.*&b=download/);
  await compare.click();
  await expect(
    page.getByRole("table").getByRole("rowheader", { name: "Sample rate" })
  ).toBeVisible();
  await page.keyboard.press("Escape");

  // the two views move together until they are unlinked
  const view = (slot: Locator) => slot.getByRole("slider", { name: "Cursor" });
  await a.getByRole("button", { name: "Zoom in" }).click();
  await expect(view(a)).toHaveAttribute("data-view", "0.000 15.250");
  await expect(view(b)).toHaveAttribute("data-view", "0.000 15.250");
  // a file shown again beside the other takes that one's view, and leaves it as it was
  await a.getByRole("button", { name: "Clear slot A" }).click();
  await expect(page).not.toHaveURL(/\?a=/);
  await page.goBack();
  await expect(view(a)).toHaveAttribute("data-view", "0.000 15.250");
  await expect(view(b)).toHaveAttribute("data-view", "0.000 15.250");
  await page.getByText("Link zoom and scroll").click();
  await expect(page.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  const held = await view(b).getAttribute("data-view");
  await a.getByRole("button", { name: "Zoom in" }).click();
  await expect(view(a)).not.toHaveAttribute("data-view", held!);
  await expect(view(b)).toHaveAttribute("data-view", held!);

  // a reload restores both, from the address
  await page.reload();
  await expect(view(a)).toBeVisible();
  await expect(view(b)).toBeVisible();

  // a slot cleared goes from the address too, and Back brings it back
  await b.getByRole("button", { name: "Clear slot B" }).click();
  await expect(b.getByRole("button", { name: "Add from Downloads" })).toBeVisible();
  await expect(b).toBeFocused();
  await expect(page).not.toHaveURL(/&b=/);
  await page.goBack();
  await expect(view(b)).toBeVisible();
  await a.getByRole("button", { name: "Clear slot A" }).click();
  await b.getByRole("button", { name: "Clear slot B" }).click();
  await expect(a.getByText("Add a file to analyze")).toBeVisible();
  await expect(b.getByText("Add a file to compare")).toBeVisible();
  await expect(page).toHaveURL(/#\/analysis$/);
});

test("a local file is tagged and unjudged, and the plots keep a readable height", async ({
  page
}) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await page.setViewportSize({ width: 1280, height: 720 });
  await visit(page, "/analysis");
  const a = page.getByRole("region", { name: "Slot A" });
  const b = page.getByRole("region", { name: "Slot B" });
  const scrolls = () =>
    page.locator("[data-content]").evaluate((e) => e.scrollHeight - e.clientHeight);
  const height = async (box: Locator) => (await box.boundingBox())!.height;
  const upload = (slot: Locator, letter: string) =>
    slot
      .getByLabel(`Upload a file to slot ${letter}`)
      .setInputFiles({ name: "mine.flac", mimeType: "audio/flac", buffer: Buffer.alloc(2048) });
  // with nothing loaded both rows are there, half each, and the page does not scroll
  await expect(a.getByRole("button", { name: "Upload file" })).toBeVisible();
  await expect(b.getByRole("button", { name: "Upload file" })).toBeVisible();
  expect(await height(a)).toBe(await height(b));
  expect(await scrolls()).toBe(0);

  // a plot is at least as tall as a phone shows it, and a short window scrolls to make room
  await upload(a, "A");
  const plotA = a.getByRole("slider", { name: "Cursor" });
  await expect(plotA).toBeVisible();
  // a file from this device is marked as one, has no judgment and stays out of the address
  await expect(a.getByText("Local file")).toBeVisible();
  await expect(a.getByRole("radiogroup", { name: "Your judgment" })).toHaveCount(0);
  await expect(page).toHaveURL(/#\/analysis$/);
  await page.setViewportSize({ width: 390, height: 844 });
  const phone = await height(plotA);
  await page.setViewportSize({ width: 1280, height: 720 });
  expect(await height(plotA)).toBeGreaterThanOrEqual(phone);
  expect(await scrolls()).toBeGreaterThan(0);
  await upload(b, "B");
  const plotB = b.getByRole("slider", { name: "Cursor" });
  await expect(plotB).toBeVisible();
  expect(await height(plotB)).toBeGreaterThanOrEqual(phone);
  expect(await height(a)).toBe(await height(b));

  // a window tall enough for both splits it in half, and the plots grow with it
  const short = await height(plotA);
  await page.setViewportSize({ width: 1280, height: 1600 });
  await expect.poll(() => height(plotA)).toBeGreaterThan(short + 100);
  expect(await height(a)).toBe(await height(b));
  expect(await scrolls()).toBe(0);
});

test("picks a file from the panel of the daemon's files", async ({ page }) => {
  await fakeAnalysis(page);
  await page.route("**/api/v0/files/downloads/directories?recursive=true", (route) =>
    route.fulfill({
      json: listedFolder("", "downloads", {
        directories: [],
        files: [
          listedFile("track.flac"),
          listedFile("Album/01.flac"),
          listedFile("Album/cover.jpg")
        ]
      })
    })
  );
  await visit(page, "/analysis");
  const a = page.getByRole("region", { name: "Slot A" });
  await a.getByRole("button", { name: "Add from Downloads" }).click();
  const tree = a.getByRole("treegrid");
  await expect(tree.getByText("track.flac")).toBeVisible();
  await a.getByRole("textbox", { name: "Filter files" }).fill("01");
  await expect(tree.getByText("01.flac")).toBeVisible();
  await expect(tree.getByText("track.flac")).toHaveCount(0);
  await tree.getByText("01.flac").click();
  await expect(tree).toHaveCount(0);
  await expect(a).toBeFocused();
  await expect(page).toHaveURL(/#\/analysis\?a=downloads%3AAlbum%2F01\.flac$/);
});
