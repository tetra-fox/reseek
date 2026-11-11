import { expect, it, vi } from "vitest";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$app/state", () => ({
  page: { url: new URL("http://localhost/#/dashboard") },
  navigating: { to: null }
}));

/** The module as a fresh page reads it, with `saved` in this browser */
const load = async (saved: Record<string, string> = {}) => {
  // what an earlier copy has still to write lands first
  await new Promise((done) => setTimeout(done));
  localStorage.setItem("reseek.screens", JSON.stringify(saved));
  vi.resetModules();
  return import("./screens.svelte");
};

it("brings System and Users back to their tab and filters, not to a profile", async () => {
  const { screenHref, visited } = await load();
  expect(screenHref("/system")).toBe("#/system");
  visited("/system/settings", new URLSearchParams("q=port"));
  expect(screenHref("/system")).toBe("#/system/settings?q=port");

  visited("/users", new URLSearchParams("tab=buddies"));
  visited("/users/bob/shares", new URLSearchParams("path=Music"));
  expect(screenHref("/users")).toBe("#/users?tab=buddies");

  visited("/downloads", new URLSearchParams());
  expect(screenHref("/downloads")).toBe("#/downloads");
});

it("keeps only System's own tabs, which an unknown one never loops back to", async () => {
  const { leftAt, visited } = await load();
  visited("/system/logs", new URLSearchParams());
  // a mistyped tab is sent to System, which must not send it back
  visited("/system/nonsense", new URLSearchParams());
  visited("/system/", new URLSearchParams());
  expect(leftAt("/system")).toBe("/system/logs");

  const saved = await load({ "/system": "/system/nonsense", "/users": "/users?page=2" });
  expect(saved.leftAt("/system")).toBeNull();
  expect(saved.leftAt("/users")).toBe("/users?page=2");
  // a saved address with its own `#` still links once
  const marked = await load({ "/system": "#/system/logs" });
  expect(marked.screenHref("/system")).toBe("#/system/logs");
});
