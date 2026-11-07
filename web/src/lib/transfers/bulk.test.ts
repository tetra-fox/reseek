import { describe, expect, it } from "vitest";

import { STATES, transfer } from "#lib/testing/fixtures.js";

import { activeAmong, bulkRequest, summary } from "./bulk";

describe("bulk actions", () => {
  const rows = STATES.map((state) => transfer({ state }));
  const ids = (states: string[]) => rows.filter((t) => states.includes(t.state)).map((t) => t.id);

  it("send only the transfers an action applies to", () => {
    expect(bulkRequest("retry", rows)).toEqual({
      action: "retry",
      ids: ids(STATES.filter((s) => s.startsWith("Completed, ") && s !== "Completed, Succeeded"))
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
