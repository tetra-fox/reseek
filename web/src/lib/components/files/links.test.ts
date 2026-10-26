import { describe, expect, it } from "vitest";

import { isAudio, remoteFolder } from "./links";

describe("file links", () => {
  it("point Browse at the folder a remote file is in", () => {
    expect(remoteFolder("al ice", "Music\\Artist\\01.flac")).toEqual({
      folder: "Music\\Artist",
      browse: "#/browse?user=al%20ice&path=Music%5CArtist",
      flat: "#/browse?user=al%20ice&path=Music%5CArtist&flat=1"
    });
  });

  it("tell audio by its extension", () => {
    expect(isAudio("a/b.FLAC")).toBe(true);
    expect(isAudio("a/b.jpg")).toBe(false);
  });
});
