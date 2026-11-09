import { afterEach, describe, expect, it, vi } from "vitest";

import type { Layout } from "#lib/analysis/analysis.js";

import { Pictures, tilesFit } from "./pictures.svelte";

const layout: Layout = {
  scale: "linear",
  maxHz: 22_050,
  durationSeconds: 300,
  sampled: false,
  guides: [],
  bins: 512,
  columnsPerSecond: 16,
  peaksPerSecond: 200,
  tileSeconds: 16,
  tiles: 19
};

afterEach(() => vi.unstubAllGlobals());

describe("Pictures", () => {
  it("asks for a tile that failed once, and says why", async () => {
    // the kept file is gone: every tile answers 404
    const asked: string[] = [];
    vi.stubGlobal("fetch", (url: string) => {
      asked.push(url);
      return Promise.resolve(
        new Response(JSON.stringify({ error: "the spectrogram is no longer kept" }), {
          status: 404,
          headers: { "Content-Type": "application/json" }
        })
      );
    });
    const pictures = new Pictures({ kind: "download", username: "alice", id: "x" }, layout);
    pictures.want(0, 20);
    await vi.waitFor(() => expect(pictures.lost.tile).toBe("the spectrogram is no longer kept"));
    await vi.waitFor(() => expect(asked).toHaveLength(4));
    // a pan over the same stretch, and on to the next, asks only for what was not asked before
    for (let i = 0; i < 10; i++) pictures.want(i, 20 + i);
    pictures.want(30, 40);
    await vi.waitFor(() => expect(asked).toHaveLength(6));
    expect(asked.filter((url) => url.endsWith("tile=0"))).toHaveLength(2);
  });

  it("asks for no more tiles than it keeps, however many the file has", async () => {
    const asked: string[] = [];
    vi.stubGlobal("fetch", (url: string) => {
      asked.push(url);
      return Promise.resolve(new Response(null, { status: 404 }));
    });
    const tiles = 2 ** 32 - 1;
    const huge = { ...layout, durationSeconds: tiles * 16, tiles };
    const pictures = new Pictures({ kind: "download", username: "alice", id: "x" }, huge);
    pictures.want(0, huge.durationSeconds);
    await vi.waitFor(() => expect(asked).toHaveLength(96));
    // the tiles kept are those around the middle of what was asked for
    const middle = Math.floor(tiles / 2);
    expect(asked.some((url) => url.endsWith(`tile=${middle}`))).toBe(true);
    expect(asked.some((url) => url.endsWith("tile=0"))).toBe(false);
  });

  it("leaves a view wider than the kept tiles to the overview", () => {
    const long = { ...layout, durationSeconds: 3600, tiles: 225 };
    // 48 tiles of 16 s: from the start of tile 0 to the end of tile 47
    expect(tilesFit(long, 0, 48 * 16 - 1)).toBe(true);
    expect(tilesFit(long, 0, 48 * 16)).toBe(false);
    expect(tilesFit(long, 8, 48 * 16 + 7)).toBe(false);
    // past the end of the file counts only the tiles it has
    expect(tilesFit(layout, -100, 1000)).toBe(true);
  });
});
