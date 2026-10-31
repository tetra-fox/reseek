import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { confirmation } from "#lib/feedback/confirm.svelte.js";
import { session } from "#lib/session.svelte.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Files from "./Files.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const flush = async () => {
  for (let i = 0; i < 3; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
};

/** Answers the screen's reads of the analysis reports and queue with none, and the rest by `listing` */
const withoutAnalyses = (listing: (url: URL) => unknown) => (url: URL) =>
  url.pathname.includes("/analysis") ? [] : listing(url);

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
    fakeNetwork(
      withoutAnalyses(() => ({
        name: "Album",
        fullName: "",
        directories: [{ name: "CD1", fullName: "CD1", modifiedAt: "2026-10-26T10:00:00Z" }],
        files: [
          { name: "a.flac", fullName: "a.flac", length: 3, modifiedAt: "2026-10-26T10:00:00Z" },
          { name: "b.flac", fullName: "b.flac", length: 3, modifiedAt: "2026-10-26T10:00:00Z" }
        ]
      }))
    );
    component = mount(Files, {
      target: document.body,
      props: { root: "downloads", path: "Album" }
    });
    await flush();
    const grid = document.querySelector("[role=treegrid]")!;
    press(grid, "ArrowDown");
    expect(active(grid)).toBe("..");
    press(grid, "ArrowDown");
    press(grid, "Enter");
    expect(goto).toHaveBeenCalledWith("#/system/files?root=downloads&path=Album%2FCD1");
    press(grid, "ArrowDown");
    press(grid, " ", { shiftKey: true });
    press(grid, "ArrowDown");
    press(grid, " ", { shiftKey: true });
    expect(ticked()).toEqual(["f:Album/a.flac", "f:Album/b.flac"]);
    press(grid, "Delete");
    expect(confirmation.question?.title).toBe("Delete 2 items?");
  });

  it("opens a folder in place, reading it, and closes it with the keys", async () => {
    const listings: Record<string, unknown> = {
      "Album": {
        name: "Album",
        fullName: "",
        directories: [{ name: "CD1", fullName: "CD1", modifiedAt: "2026-10-26T10:00:00Z" }],
        files: []
      },
      "Album/CD1": {
        name: "CD1",
        fullName: "",
        directories: [],
        files: [
          { name: "1.flac", fullName: "1.flac", length: 3, modifiedAt: "2026-10-26T10:00:00Z" }
        ]
      }
    };
    const net = fakeNetwork(
      withoutAnalyses((url) => {
        const encoded = url.pathname.split("/directories/")[1];
        const path = new TextDecoder().decode(
          Uint8Array.from(atob(encoded.replaceAll("-", "+").replaceAll("_", "/")), (c) =>
            c.charCodeAt(0)
          )
        );
        return listings[path];
      })
    );
    component = mount(Files, {
      target: document.body,
      props: { root: "downloads", path: "Album" }
    });
    await flush();
    const grid = document.querySelector("[role=treegrid]")!;
    const rows = () =>
      [...grid.querySelectorAll("[role=row][data-line]")].map((r) => r.getAttribute("data-line"));
    expect(rows()).toEqual(["..", "folder:Album/CD1"]);
    press(grid, "ArrowDown");
    press(grid, "ArrowDown");
    press(grid, "ArrowRight");
    await flush();
    expect(rows()).toEqual(["..", "folder:Album/CD1", "f:Album/CD1/1.flac"]);
    // every row has its cells, and a folder its level and whether it is open
    for (const row of grid.querySelectorAll("[role=row][data-line]")) {
      expect(row.querySelector("[role=gridcell]")).not.toBeNull();
    }
    const folder = grid.querySelector('[data-line="folder:Album/CD1"]')!;
    expect([folder.getAttribute("aria-level"), folder.getAttribute("aria-expanded")]).toEqual([
      "1",
      "true"
    ]);
    expect(grid.querySelector('[data-line="f:Album/CD1/1.flac"]')!.getAttribute("aria-level")).toBe(
      "2"
    );
    // one listing per folder: the open folder, then the one opened in place
    expect(net.requests.filter((r) => r.url.pathname.includes("/directories/"))).toHaveLength(2);
    press(grid, "ArrowLeft");
    expect(rows()).toEqual(["..", "folder:Album/CD1"]);
  });

  it("says a subfolder holding nothing is empty, its way up shown", async () => {
    fakeNetwork(
      withoutAnalyses(() => ({ name: "Empty", fullName: "", directories: [], files: [] }))
    );
    component = mount(Files, {
      target: document.body,
      props: { root: "downloads", path: "Album/Empty" }
    });
    await flush();
    const rows = [...document.querySelectorAll("[role=row][data-line]")];
    expect(rows.map((r) => r.getAttribute("data-line"))).toEqual([".."]);
    expect(document.body.textContent).toContain("This folder is empty");
  });
});
