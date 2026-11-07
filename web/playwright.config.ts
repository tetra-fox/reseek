import { execFileSync } from "node:child_process";

import { defineConfig } from "@playwright/test";

import { SESSION } from "./e2e/support/session";

const port = Number(process.env.SMOKE_PORT ?? 5139);
/** the daemon with real transfers, logged in to a soulfind of its own when there is one */
const networkPort = port + 10;

/** The browser to drive: CHROMIUM_BIN, else a chromium on PATH */
const chromium = (): string => {
  if (process.env.CHROMIUM_BIN) return process.env.CHROMIUM_BIN;
  try {
    return execFileSync("sh", ["-c", "command -v chromium"], { encoding: "utf8" }).trim();
  } catch {
    throw new Error("no browser for the smoke test: set CHROMIUM_BIN or put chromium on PATH");
  }
};
const launchOptions = { executablePath: chromium() };

/** The daemon on localhost, where the tests start with the session the login project saved */
const local = {
  baseURL: `http://127.0.0.1:${port}/reseek/`,
  launchOptions,
  storageState: SESSION
};

/**
 * How the servers are stopped: SIGTERM lets serve.sh remove its directory; without it playwright
 * sends SIGKILL, which no trap sees
 */
const gracefulShutdown = { signal: "SIGTERM", timeout: 10_000 } as const;

export default defineConfig({
  testDir: "e2e",
  testMatch: "**/*.e2e.ts",
  retries: 0,
  // the tests share one daemon, and one of them changes its configuration
  workers: 1,
  projects: [
    {
      // logs in through the form once; the daemon lives for the whole run, so its session does
      name: "login",
      testMatch: "**/login.setup.ts",
      use: { baseURL: local.baseURL, launchOptions }
    },
    {
      name: "localhost",
      testIgnore: ["**/insecure.e2e.ts", "**/network.e2e.ts"],
      dependencies: ["login"],
      use: local
    },
    {
      // the frame-rate benchmarks, which `pnpm perf` runs and the smoke test leaves out
      name: "perf",
      testMatch: "**/*.perf.ts",
      dependencies: ["login"],
      use: local
    },
    {
      // plain http on a name that is not localhost, as on a LAN address, is no secure context.
      // the session cookie is 127.0.0.1's, so these log in through the form
      name: "insecure",
      testMatch: "**/insecure.e2e.ts",
      use: {
        baseURL: `http://reseek.test:${port}/reseek/`,
        launchOptions: {
          ...launchOptions,
          args: ["--host-resolver-rules=MAP reseek.test 127.0.0.1"]
        }
      }
    },
    {
      // a daemon of its own, which the saved session is not one of
      name: "network",
      testMatch: "**/network.e2e.ts",
      use: { baseURL: `http://127.0.0.1:${networkPort}/reseek/`, launchOptions }
    }
  ],
  webServer: [
    {
      command: "./e2e/support/serve.sh",
      url: `http://127.0.0.1:${port}/reseek/`,
      reuseExistingServer: false,
      gracefulShutdown,
      env: { SMOKE_PORT: String(port) }
    },
    {
      command: "./e2e/support/serve.sh",
      url: `http://127.0.0.1:${networkPort}/reseek/`,
      reuseExistingServer: false,
      gracefulShutdown,
      env: { SMOKE_PORT: String(networkPort), SMOKE_NETWORK: "1" }
    }
  ]
});
