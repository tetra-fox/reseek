import { expect, test } from "@playwright/test";

import { fakeAnalysis, fakeDownloads } from "./fakes";
import { login } from "./login";

test("analyses a finished download from its row and keeps a judgement", async ({ page }) => {
  await fakeDownloads(page);
  await fakeAnalysis(page);
  await login(page, "/downloads");
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
  // the plot's columns start 48 of the image's 300 px in and run 30 px, one a window
  await plot.click({ position: { x: ((48 + 10.5) / 300) * box.width, y: box.height / 2 } });
  await expect(drawer.getByText("Spectrum at 10s")).toBeInViewport();
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
        const id = crypto.randomUUID();
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
  // the queue names its files by the daemon's own path
  await page.route("**/api/v0/analysis/jobs", (route) =>
    route.fulfill({
      json: queued.map(({ id }, i) => ({
        id,
        path: `/srv/downloads/${i}`,
        transferId: null,
        state: "waiting",
        queuedAt: at,
        startedAt: null,
        progress: null
      }))
    })
  );
  await login(page, "/system/files");
  await expect(page.getByRole("button", { name: "More for notes.txt" })).toBeVisible();
  await page.getByRole("button", { name: "More for notes.txt" }).click();
  await expect(page.getByRole("menuitem", { name: "Analyse" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "More for track.flac" }).click();
  await page.getByRole("menuitem", { name: "Analyse" }).click();
  const drawer = page.getByRole("complementary", { name: "Analysis" });
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
