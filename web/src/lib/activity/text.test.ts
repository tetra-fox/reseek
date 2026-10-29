import { describe, expect, it } from "vitest";

import type { ActivityEntry } from "./activity.svelte";
import { activityParts, activityText, kindOf } from "./text";

const size = (bytes: number) => `${bytes} B`;

const entry = (kind: ActivityEntry["kind"], username: string | null = "alice"): ActivityEntry => ({
  id: 1,
  at: "2026-10-27T18:00:00Z",
  username,
  kind
});

describe("an activity line", () => {
  it("says what another user did in plain words", () => {
    expect(activityText(entry({ type: "browse" }), size)).toBe("alice is browsing your shares");
    expect(
      activityText(entry({ type: "search", query: "boards of canada", results: 12 }), size)
    ).toBe('alice searched for "boards of canada" and got 12 results');
    expect(activityText(entry({ type: "search", query: "x", results: 1 }), size)).toBe(
      'alice searched for "x" and got 1 result'
    );
    expect(
      activityText(
        entry({ type: "queued", files: 3, filename: "@@a\\Music\\Album\\03 x.flac" }),
        size
      )
    ).toBe("alice queued 3 files, the last 03 x.flac");
    expect(
      activityText(
        entry({
          type: "uploadEnded",
          filename: "a/b.mp3",
          state: "Completed, Cancelled",
          bytes: 1
        }),
        size
      )
    ).toBe("Upload of b.mp3 to alice canceled after 1 B");
    expect(
      activityText(entry({ type: "privateRoom", room: "club", change: "added" }, null), size)
    ).toBe("You were added to the private room club");
  });

  it("names the user, the file and the room as their own runs, for the dock's links", () => {
    const parts = activityParts(
      entry({ type: "queued", files: 2, filename: "Music\\a.flac" }),
      size
    );
    expect(
      parts.filter((p) => p.kind !== "text" && p.kind !== "strong" && p.kind !== "muted")
    ).toEqual([
      { kind: "user", name: "alice" },
      { kind: "file", path: "Music\\a.flac" }
    ]);
    expect(
      activityParts(entry({ type: "privateRoom", room: "club", change: "operator" }, null), size)
    ).toContainEqual({ kind: "room", name: "club" });
  });

  it("files an upload's end with its start", () => {
    expect(kindOf("uploadEnded")).toBe("uploadStarted");
    expect(kindOf("browse")).toBe("browse");
  });
});
