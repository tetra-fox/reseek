import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { confirmation } from "#lib/confirm.svelte.js";
import { session } from "#lib/session.svelte.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Files from "./Files.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const flush = async () => {
  for (let i = 0; i < 3; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

describe("the file explorer", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    session.principal = null;
    app.options = null;
    confirmation.open = false;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("walks the listing by key, opens a directory with Enter and deletes the selection", async () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.options = { remote_file_management: true } as never;
    fakeNetwork(() => ({
      name: "Album",
      fullName: "",
      directories: [{ name: "CD1", fullName: "CD1", modifiedAt: "2026-10-26T10:00:00Z" }],
      files: [
        { name: "a.flac", fullName: "a.flac", length: 3, modifiedAt: "2026-10-26T10:00:00Z" },
        { name: "b.flac", fullName: "b.flac", length: 3, modifiedAt: "2026-10-26T10:00:00Z" }
      ]
    }));
    component = mount(Files, {
      target: document.body,
      props: { root: "downloads", path: "Album" }
    });
    await flush();
    const grid = document.querySelector("[role=grid]")!;
    press(grid, "ArrowDown");
    expect(active(grid)).toBe("..");
    press(grid, "ArrowDown");
    press(grid, "Enter");
    expect(goto).toHaveBeenCalledWith("#/system/files?root=downloads&path=Album%2FCD1");
    press(grid, "ArrowDown");
    press(grid, " ");
    press(grid, "ArrowDown");
    press(grid, " ");
    expect(ticked()).toEqual(["f:a.flac", "f:b.flac"]);
    press(grid, "Delete");
    expect(confirmation.question?.body).toBe("Delete 2 items? This cannot be undone.");
  });
});
