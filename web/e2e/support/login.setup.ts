import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

import { test as setup } from "@playwright/test";

import { login } from "./login";
import { SESSION } from "./session";

setup("logs in once for the tests that start logged in", async ({ page }) => {
  await login(page);
  // the cookie alone, so each test's storage starts empty, as a new browser's does
  const state = await page.context().storageState();
  await mkdir(dirname(SESSION), { recursive: true });
  await writeFile(SESSION, JSON.stringify({ ...state, origins: [] }));
});
