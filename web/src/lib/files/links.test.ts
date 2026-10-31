import { describe, expect, it } from "vitest";

import { remoteFolder } from "./links";

describe("file links", () => {
  it("point a user's Shares tab at the folder a remote file is in", () => {
    expect(remoteFolder("al ice", "Music\\Artist\\01.flac")).toEqual({
      folder: "Music\\Artist",
      browse: "#/users/al%20ice/shares?path=Music%5CArtist"
    });
  });
});
