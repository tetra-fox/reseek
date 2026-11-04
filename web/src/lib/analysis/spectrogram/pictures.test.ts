import { afterEach, describe, expect, it, vi } from "vitest";

import type { Layout } from "#lib/analysis/analysis.js";

import { Pictures } from "./pictures.svelte";

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
    await vi.waitFor(() => expect(pictures.lost).toBe("the spectrogram is no longer kept"));
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
});
