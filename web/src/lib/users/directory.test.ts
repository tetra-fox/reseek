import { describe, expect, it } from "vitest";

import { directoryAt, withDirectory } from "./directory";

describe("the directory's address", () => {
  it("reads back every filter it writes, and leaves out what is off", () => {
    const at = {
      ...directoryAt(new URLSearchParams("tab=all&page=4")),
      q: "ann",
      blocked: true,
      presence: "Online" as const,
      hasTransfers: true
    };
    const written = withDirectory(new URLSearchParams("tab=all&page=4"), at);
    expect(written.toString()).toBe("tab=all&page=4&q=ann&blocked=1&presence=Online&has=transfers");
    expect(directoryAt(written)).toEqual(at);
  });
});
