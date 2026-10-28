import { expect, test } from "@playwright/test";

import { newId } from "../src/lib/ids";
import { fakeAnalysis, fakeDownloads } from "./fakes";
import { login } from "./login";

test("analyses a finished download from its row and keeps a judgement", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
  const table = page.getByRole("grid", { name: "Downloads" });
  // the analysed download says so in its row, muted, with the reading in its tooltip
  await expect(table.getByText("Analysed", { exact: true })).toHaveCount(1);
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyse" }).click();

  const drawer = page.getByRole("complementary", { name: "Transfer details" });
  await expect(drawer.getByRole("heading", { name: "Analysis" })).toBeInViewport();
  await expect(drawer.getByRole("img", { name: /^Spectrogram/ })).toBeVisible();
  await expect(drawer.getByText(/This is a tool to help you judge, not proof/)).toBeVisible();
  // each guide line shows and hides from its legend
  const line = drawer.locator('line[data-guide="MP3 128k"]');
  await expect(line).toHaveCount(1);
  const legend = drawer.getByRole("button", { name: "MP3 128k" });
  await legend.click();
  await expect(legend).toHaveAttribute("aria-pressed", "false");
  await expect(line).toHaveCount(0);
  await legend.click();
  await expect(line).toHaveCount(1);

  // the user's judgement, saved with its note and a tick
  await drawer.getByRole("radio", { name: "Fake" }).click();
  await drawer.getByLabel("Note").fill("sounds like an old rip");
  await drawer.getByRole("button", { name: "Save judgement" }).click();
  await expect(drawer.getByRole("status").filter({ hasText: "Saved" })).toBeVisible();
  await expect(drawer.getByRole("button", { name: "Save judgement" })).toBeDisabled();
  // the row now shows the user's own judgement
  await expect(table.getByText("Fake", { exact: true })).toBeVisible();

  // the analyser's reading, folded away and labelled as its own; its window strip picks a
  // window by key and shows that window's spectrum
  await drawer.getByText("The analyser's reading").click();
  await expect(drawer.getByText("Likely a transcode")).toBeVisible();
  const strip = drawer.getByRole("slider", { name: "Window" });
  await strip.focus();
  await page.keyboard.press("ArrowRight");
  await expect(strip).toHaveAttribute("aria-valuetext", /^0s, stops at/);
  await expect(drawer.getByRole("img", { name: /The spectrum of the window at/ })).toBeVisible();
  await drawer.getByRole("button", { name: "Next window" }).click();
  await expect(drawer.getByText("Spectrum at 1s")).toBeVisible();
  await strip.press("End");
  await expect(drawer.getByText("Spectrum at 29s")).toBeVisible();

  // a click on the plot, with the reading folded, opens it at that moment's spectrum
  await drawer.getByText("The analyser's reading").click();
  await expect(strip).toBeHidden();
  const plot = drawer.getByRole("img", { name: /^Spectrogram/ });
  const box = (await plot.boundingBox())!;
  // the plot starts 48 of the image's 1294 px in and spans the file's 30.5 s over 1024 px
  const atTime = (t: number) => ((48 + (t / 30.5) * 1024) / 1294) * box.width;
  await plot.click({ position: { x: atTime(10.5), y: box.height / 2 } });
  await expect(drawer.getByText("Spectrum at 10s")).toBeInViewport();

  // the frequency scale is the viewer's to choose, kept in this browser
  const scales = drawer.getByRole("radiogroup", { name: "Frequency scale" });
  await expect(scales.getByRole("radio", { name: "Linear" })).toHaveAttribute(
    "aria-checked",
    "true"
  );
  await scales.getByRole("radio", { name: "Mel" }).click();
  await expect(scales.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");
  await expect(plot).toHaveAttribute("src", /scale=mel/);
  // the picked window and the open reading stay as they were
  await expect(drawer.getByText("Spectrum at 10s")).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "More for 2.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyse" }).click();
  await expect(scales.getByRole("radio", { name: "Mel" })).toHaveAttribute("aria-checked", "true");

  // and the whole file can be seen across the screen
  await drawer.getByRole("button", { name: "Open large" }).click();
  const large = page.getByRole("dialog", { name: "2.flac" });
  await expect(large.getByRole("img", { name: /^Spectrogram/ })).toBeVisible();
  const wide = (await large.getByRole("img", { name: /^Spectrogram/ }).boundingBox())!;
  expect(wide.width).toBeGreaterThan(1000);
  await page.keyboard.press("Escape");
  await expect(large).toBeHidden();
});

test("filters downloads by their analysis, by chip and by word", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
  const table = page.getByRole("grid", { name: "Downloads" });
  const shown = (name: string) => table.getByRole("button", { name: `More for ${name}` });
  await expect(shown("3.flac")).toBeVisible();

  const analysis = page.getByRole("group", { name: "Analysis" });
  await analysis.getByRole("button", { name: /^Analysed/ }).click();
  await expect(shown("2.flac")).toBeVisible();
  await expect(shown("3.flac")).toHaveCount(0);
  // a finished flac without a report is waiting to be analysed; the running one is not
  await analysis.getByRole("button", { name: /^Not analysed/ }).click();
  await expect(shown("3.flac")).toBeVisible();
  await expect(shown("2.flac")).toHaveCount(0);
  await expect(shown("1.flac")).toHaveCount(0);
  await analysis.getByRole("button", { name: /^Not analysed/ }).click();

  await page.getByRole("searchbox", { name: "Filter" }).fill("analysed:yes");
  await expect(shown("2.flac")).toBeVisible();
  await expect(shown("3.flac")).toHaveCount(0);
  await page.getByRole("searchbox", { name: "Filter" }).fill("marked:fake");
  await expect(shown("2.flac")).toHaveCount(0);
});

test("analyses a file and queues a folder from the Files screen", async ({ page }) => {
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
        return route.fulfill({ status: 404, json: { error: "the file has not been analysed" } });
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
  await expect(page.getByRole("menuitem", { name: "Analyse" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  // the analysed file says so, and the filter by the analyser's reading finds it
  const files = page.getByRole("treegrid");
  await expect(files.getByText("Analysed", { exact: true })).toBeVisible();
  const show = page.getByRole("combobox", { name: "Show" });
  await show.click();
  await page.getByRole("option", { name: "Analyser: likely a transcode" }).click();
  await expect(page.getByRole("button", { name: "More for track.flac" })).toBeVisible();
  await show.click();
  await page.getByRole("option", { name: "Not analysed" }).click();
  await expect(page.getByRole("button", { name: "More for track.flac" })).toHaveCount(0);
  await show.click();
  await page.getByRole("option", { name: "All files" }).click();

  await page.getByRole("button", { name: "More for track.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyse" }).click();
  const drawer = page.getByRole("complementary", { name: "Analysis" });
  // queued by another browser, it shows its place in the queue
  await expect(drawer.getByText("Waiting for the analyser")).toBeVisible();
  await expect(drawer.getByRole("img", { name: /^Spectrogram/ })).toBeVisible();
  await expect(drawer.getByText(/This is a tool to help you judge, not proof/)).toBeVisible();
  // the file's own actions sit beside its name
  await expect(drawer.getByRole("button", { name: "Preview" })).toBeVisible();
  await expect(drawer.getByRole("button", { name: "Download to device" })).toBeVisible();
  await drawer.getByRole("button", { name: "Close the analysis" }).click();

  await page.getByRole("button", { name: "More for Album" }).click();
  await page.getByRole("menuitem", { name: "Analyse folder" }).click();
  await expect(page.getByText("2 files queued for analysis")).toBeVisible();
  expect(queued.map((q) => q.path)).toEqual(["Album/01.flac", "Album/CD2/02.wav"]);

  // a file the folder queued shows its place in the queue, not an offer to analyse it
  await page.getByRole("button", { name: "More for Album" }).click();
  await page.getByRole("menuitem", { name: "Open" }).click();
  await page.getByRole("button", { name: "More for 01.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyse" }).click();
  await expect(drawer.getByText("Waiting for the analyser")).toBeVisible();
  await expect(drawer.getByRole("button", { name: "Analyse", exact: true })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Analysis queue" })).toBeVisible();
});
