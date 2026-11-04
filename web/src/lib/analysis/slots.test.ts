import { goto } from "$app/navigation";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { toasts } from "#lib/feedback/toast.svelte.js";

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
    toasts.splice(0);
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

  it("moves the address only when a change gives it a new one", () => {
    slots.clear("a");
    expect(goto).not.toHaveBeenCalled();

    // from another screen a list opens the page as a new screen
    show("#/downloads");
    slots.open(file("one.flac"), "one.flac");
    expect(goto).toHaveBeenLastCalledWith("#/analysis?a=downloads%3Aone.flac", {
      reset: true,
      replace: false
    });

    // on the page the same file again is no move at all, another one keeps scroll and focus
    show("#/analysis?a=downloads%3Aone.flac");
    vi.mocked(goto).mockClear();
    slots.fill("a", file("one.flac"), "one.flac");
    expect(goto).not.toHaveBeenCalled();
    slots.fill("b", file("two.flac"), "two.flac");
    expect(goto).toHaveBeenLastCalledWith(
      "#/analysis?a=downloads%3Aone.flac&b=downloads%3Atwo.flac",
      { reset: false, replace: false }
    );
  });

  it("goes by the address a move still on its way is headed to", () => {
    show("#/analysis?a=downloads%3Aone.flac&b=downloads%3Atwo.flac");
    slots.a = { target: file("one.flac"), name: null };
    slots.b = { target: file("two.flac"), name: null };
    slots.swap();
    expect(goto).toHaveBeenCalledTimes(1);
    at.navigating.to = { url: new URL(`http://localhost/${vi.mocked(goto).mock.calls[0][0]}`) };

    // a second swap before the first lands returns to the address shown, in place of the move
    slots.swap();
    expect(goto).toHaveBeenLastCalledWith(
      "#/analysis?a=downloads%3Aone.flac&b=downloads%3Atwo.flac",
      { reset: false, replace: true }
    );
  });

  it("sends only a file the analyzer reads", () => {
    slots.upload("a", new File(["x"], "cover.jpg"));
    expect(sendFile).not.toHaveBeenCalled();
    expect(toasts[0]?.text).toBe(
      "cover.jpg cannot be analyzed, as it is not a type the analyzer reads."
    );
  });
});
