import { expect, it } from "vitest";

import { userFacts } from "./facts";
import type { SearchResponse } from "./results";

const response = (queueLength: number): SearchResponse => ({
  username: "alice",
  token: 1,
  fileCount: 0,
  files: [],
  lockedFileCount: 0,
  lockedFiles: [],
  hasFreeUploadSlot: false,
  uploadSpeed: 1024 ** 2,
  queueLength
});

it("writes a user's queue as long as the list's column does", () => {
  expect(userFacts(response(3))).toBe("1 MB/s · no free slot · 3 queued");
  // a peer's -1, read unsigned, would take the line from the figures before it
  expect(userFacts(response(4_294_967_295))).toBe("1 MB/s · no free slot · 99,999+ queued");
});
