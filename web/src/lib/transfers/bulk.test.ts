import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import { activeAmong, bulkRequest, summary } from "./bulk";
import { TRANSFER_STATES, type TransferState } from "./states";

describe("bulk actions", () => {
  const rows = TRANSFER_STATES.map((state) => transfer({ state }));
  const ids = (states: TransferState[]) =>
    rows.filter((t) => states.includes(t.state)).map((t) => t.id);

  it("send only the transfers an action applies to", () => {
    expect(bulkRequest("retry", rows)).toEqual({
      action: "retry",
      ids: ids([
        "Completed, Cancelled",
        "Completed, TimedOut",
        "Completed, Errored",
        "Completed, Rejected",
        "Completed, Aborted"
      ])
    });
    expect(bulkRequest("cancel", rows)).toEqual({
      action: "cancel",
      ids: ids(["Requested", "Queued, Remotely", "Queued, Locally", "Initializing", "InProgress"])
    });
    expect(bulkRequest("remove", rows)?.ids).toHaveLength(rows.length);
    expect(bulkRequest("retry", [transfer()])).toBeNull();
  });

  it("count the active transfers a cancel asks about and sum up a result", () => {
    expect(activeAmong(rows)).toBe(1);
    expect(summary("remove", { ok: ["a", "b"], failed: [{ id: "c", error: "gone" }] })).toBe(
      "2 removed, 1 failed: gone"
    );
  });
});
