/// <reference types="vitest/config" />
import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [
    tailwindcss(),
    // the hash router keeps every screen under one index.html, so one build works under any URL
    // base the daemon is given
    sveltekit({
      adapter: adapter(),
      router: { type: "hash" },
      compilerOptions: { runes: true }
    })
  ],
  // the svelte client build, which component tests mount into jsdom
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: {
    include: ["src/**/*.test.ts"],
    environment: "jsdom",
    // threads with a vm context per file: the suite takes about half the CPU it does on forks
    pool: "vmThreads",
    setupFiles: ["src/lib/testing/setup.ts"]
  },
  server: {
    // same origin as the dev UI, so the session cookie and the event stream work without CORS
    proxy: { "/api": { target: process.env.RESEEK_API ?? "http://localhost:5030" } }
  }
});
