import { goto } from "$app/navigation";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";

import type { Target } from "./analysis";
import { slots } from "./slots.svelte";
import { sendFile } from "./upload";

const at = vi.hoisted(() => ({
  page: { url: new URL("http://localhost/#/analysis") },
  navigating: { to: null as { url: URL } | null }
}));

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));
vi.mock("$app/state", () => at);
vi.mock("./upload", async (actual) => ({
  ...(await actual<typeof import("./upload")>()),
  sendFile: vi.fn(() => () => {})
}));

const show = (address: string) => (at.page.url = new URL(`http://localhost/${address}`));
const file = (path: string): Target => ({ kind: "file", root: "downloads", path });

describe("the Analysis page's slots", () => {
  beforeEach(() => {
    app.options = { analysis: { extensions: ["flac"], upload_max_bytes: "1.9 GiB" } } as never;
    show("#/analysis");
    slots.a = slots.b = null;
    vi.mocked(goto).mockClear();
    vi.mocked(sendFile).mockClear();
  });
  afterEach(() => {
    app.options = null;
    at.navigating.to = null;
  });

  it("fills a slot when an upload lands, wherever the user is", () => {
    slots.upload("a", new File(["x"], "mine.flac"));
    show("#/chat");
    const done = vi.mocked(sendFile).mock.calls[0][2];
    done({ ok: true, data: { root: "temporary", path: "upload/mine.flac" } as never });
    expect(slots.a?.target).toEqual({ kind: "file", root: "temporary", path: "upload/mine.flac" });
    expect(goto).not.toHaveBeenCalled();
  });
});
