import { defineConfig } from "@playwright/test";

import { launchOptions } from "./e2e/support/browser";
import { SESSION } from "./e2e/support/session";

/**
 * The daemon's port: E2E_PORT, else the one it picked itself, so gates run at once never share
 * one. The global setup starts it and sets E2E_PORT, which the workers find as they load this
 * file again; the runner reads it before then, and its URLs go unused
 */
const port = process.env.E2E_PORT;

/** The daemon on localhost, where the tests start with the session the login project saved */
const local = {
  baseURL: `http://127.0.0.1:${port}/reseek/`,
  launchOptions,
  storageState: SESSION
};

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
      testIgnore: "**/insecure.e2e.ts",
      dependencies: ["login"],
      use: local
    },
    {
      // the frame-rate benchmarks, which `pnpm perf` runs and `pnpm e2e` leaves out
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
    }
  ],
  globalSetup: "./e2e/support/daemon.ts"
});
