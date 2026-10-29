import { describe, expect, it } from "vitest";

import { remoteFolder } from "./links";

describe("file links", () => {
  it("point Browse at the folder a remote file is in", () => {
    expect(remoteFolder("al ice", "Music\\Artist\\01.flac")).toEqual({
      folder: "Music\\Artist",
      browse: "#/browse?user=al%20ice&path=Music%5CArtist"
    });
  });
});
