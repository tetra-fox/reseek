import { describe, expect, it } from "vitest";

import type { Schemas } from "#lib/api/client.js";

import { quota } from "./why.svelte";

type Window = Partial<Schemas["UploadLimitWindow"]>;

describe("an upload group's quota", () => {
  const status = (daily: Window, weekly: Window): Schemas["UploadQueueStatus"] => ({
    position: 0,
    group: "default",
    groupSlotsUsed: 0,
    groupSlots: 1,
    globalSlotsUsed: 0,
    globalSlots: 1,
    userActive: 0,
    limits: {
      queued: { files: { used: 0 }, bytes: { used: 0 } },
      daily: { files: { used: 0 }, bytes: { used: 0 }, ...daily },
      weekly: { files: { used: 0 }, bytes: { used: 0 }, ...weekly }
    }
  });

  it("shows each limit against its use and is used up by any of them, failures too", () => {
    const failing = quota(
      status({ files: { used: 3, limit: 400 }, failures: { used: 30, limit: 30 } }, {})
    );
    expect(failing).toEqual({
      text: "Today: 3/400 files, 0 B, 30/30 failed · This week: 0 files, 0 B",
      exhausted: true
    });
    const sized = quota(status({ bytes: { used: 1024 ** 3, limit: 2 * 1024 ** 3 } }, {}));
    expect(sized).toEqual({
      text: "Today: 0 files, 1 GB/2 GB · This week: 0 files, 0 B",
      exhausted: false
    });
  });
});
