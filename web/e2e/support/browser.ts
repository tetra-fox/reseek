import { execFileSync } from "node:child_process";

/** The browser to drive: CHROMIUM_BIN, else a chromium on PATH */
const chromium = (): string => {
  if (process.env.CHROMIUM_BIN) return process.env.CHROMIUM_BIN;
  try {
    return execFileSync("sh", ["-c", "command -v chromium"], { encoding: "utf8" }).trim();
  } catch {
    throw new Error("no browser for the tests: set CHROMIUM_BIN or put chromium on PATH");
  }
};

/** How the tests launch the browser */
export const launchOptions = { executablePath: chromium() };
