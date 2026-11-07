import { describe, expect, it } from "vitest";

import type { ActivityEntry } from "./activity.svelte";
import { activityParts, activityText } from "./text";

const size = (bytes: number) => `${bytes} B`;

const entry = (kind: ActivityEntry["kind"], username: string | null = "alice"): ActivityEntry => ({
  id: 1,
  at: "2026-10-27T18:00:00Z",
  username,
  kind
});

describe("an activity line", () => {
  it("counts one result in the singular and tells of a change with no user as yours", () => {
    expect(activityText(entry({ type: "search", query: "x", results: 1 }), size)).toBe(
      'alice searched for "x" (1 result)'
    );
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
});
