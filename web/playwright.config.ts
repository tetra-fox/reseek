import { execFileSync } from "node:child_process";

import { defineConfig } from "@playwright/test";

const port = Number(process.env.SMOKE_PORT ?? 5139);

/** The browser to drive: CHROMIUM_BIN, else a chromium on PATH */
const chromium = (): string => {
  if (process.env.CHROMIUM_BIN) return process.env.CHROMIUM_BIN;
  try {
    return execFileSync("sh", ["-c", "command -v chromium"], { encoding: "utf8" }).trim();
  } catch {
    throw new Error("no browser for the smoke test: set CHROMIUM_BIN or put chromium on PATH");
  }
};

export default defineConfig({
  testDir: "e2e",
  testMatch: "**/*.e2e.ts",
  retries: 0,
  // the tests share one daemon, and one of them changes its configuration
  workers: 1,
  use: {
    baseURL: `http://127.0.0.1:${port}/reseek/`,
    launchOptions: { executablePath: chromium() }
  },
  webServer: {
    command: "./e2e/serve.sh",
    url: `http://127.0.0.1:${port}/reseek/`,
    reuseExistingServer: false,
    env: { SMOKE_PORT: String(port) }
  }
});
