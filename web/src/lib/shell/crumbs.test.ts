import { expect, it, vi } from "vitest";

import { baseCrumbs } from "./crumbs.svelte";
import { visited } from "./screens.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$app/state", () => ({
  page: { url: new URL("http://localhost/#/dashboard") },
  navigating: { to: null }
}));

it("links the screen's step as the navigation does, and not to the address in view", () => {
  visited("/users", new URLSearchParams("page=3"));
  // a profile's way back is the directory's page it was opened from
  expect(baseCrumbs("/users/bob")[0]).toEqual({ label: "Users", href: "#/users?page=3" });

  visited("/system/settings", new URLSearchParams("q=port"));
  expect(baseCrumbs("/system/settings", new URLSearchParams("q=port"))).toEqual([
    { label: "System", href: undefined },
    { label: "Settings", href: "#/system/settings" }
  ]);
});
