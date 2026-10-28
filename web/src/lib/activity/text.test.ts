import { describe, expect, it } from "vitest";

import type { ActivityEntry } from "./activity.svelte";
import { activityText, kindOf } from "./text";

const entry = (kind: ActivityEntry["kind"], username: string | null = "alice"): ActivityEntry => ({
  id: 1,
  at: "2026-10-27T18:00:00Z",
  username,
  kind
});

describe("an activity line", () => {
  it("says what another user did in plain words", () => {
    expect(activityText(entry({ type: "browse" }))).toBe("alice is browsing your shares");
    expect(activityText(entry({ type: "search", query: "boards of canada", results: 12 }))).toBe(
      'alice searched for "boards of canada" and got 12 results'
    );
    expect(activityText(entry({ type: "search", query: "x", results: 1 }))).toBe(
      'alice searched for "x" and got 1 result'
    );
    expect(
      activityText(entry({ type: "queued", files: 3, filename: "@@a\\Music\\Album\\03 x.flac" }))
    ).toBe("alice queued 3 files, the last 03 x.flac");
    expect(
      activityText(
        entry({ type: "uploadEnded", filename: "a/b.mp3", state: "Completed, Cancelled", bytes: 1 })
      )
    ).toBe("Upload of b.mp3 to alice ended: Cancelled");
    expect(activityText(entry({ type: "privateRoom", room: "club", change: "added" }, null))).toBe(
      "You were added to the private room club"
    );
  });

  it("files an upload's end with its start", () => {
    expect(kindOf("uploadEnded")).toBe("uploadStarted");
    expect(kindOf("browse")).toBe("browse");
  });
});
