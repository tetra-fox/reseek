import { describe, expect, it } from "vitest";

import { transfer } from "#lib/testing/fixtures.js";

import { parsePaste } from "./add";
import { activeAmong, bulkRequest, SCOPES, summary } from "./bulk";
import { rowActions, stateChip } from "./states";
import { layout, textFilter } from "./view";

const STATES = [
  "Requested",
  "Queued, Remotely",
  "Queued, Locally",
  "Initializing",
  "InProgress",
  "Completed, Succeeded",
  "Completed, Cancelled",
  "Completed, TimedOut",
  "Completed, Errored",
  "Completed, Rejected",
  "Completed, Aborted"
];

describe("the row menu", () => {
  it("offers per state what the download can take", () => {
    const enabled = (state: string, localPath: string | null = null) =>
      Object.entries(rowActions(transfer({ state, localPath }), "download"))
        .filter(([, on]) => on)
        .map(([name]) => name);
    expect(enabled("Queued, Remotely")).toEqual(["cancel", "remove", "queuePosition"]);
    expect(enabled("Queued, Locally")).toEqual(["cancel", "remove"]);
    expect(enabled("InProgress")).toEqual(["cancel", "remove"]);
    expect(enabled("Completed, Succeeded", "/dl/a.flac")).toEqual([
      "open",
      "downloadToDevice",
      "openFolder",
      "copyLocalPath",
      "remove",
      "redownload"
    ]);
    for (const state of STATES.filter(
      (s) => s.startsWith("Completed, ") && !s.endsWith("Succeeded")
    )) {
      expect(enabled(state)).toEqual(["retry", "remove", "redownload"]);
    }
  });

  it("offers deny for uploads not done and bump for queued ones", () => {
    const upload = (state: string) =>
      rowActions(transfer({ state, direction: "Upload" }), "upload");
    expect(upload("Queued, Locally")).toMatchObject({
      bump: true,
      deny: true,
      retry: false,
      cancel: true
    });
    expect(upload("InProgress")).toMatchObject({ bump: false, deny: true });
    expect(upload("Completed, Succeeded")).toMatchObject({ bump: false, deny: false, open: false });
  });
});

describe("state chips", () => {
  it("read the state, the place in the queue, the attempts and the reason", () => {
    expect(stateChip(transfer({ placeInQueue: 12 })).text).toBe("Queued (#12)");
    expect(stateChip(transfer({ state: "Requested", attempts: 3 }))).toMatchObject({
      text: "Requested ×3",
      tone: "warning"
    });
    expect(
      stateChip(transfer({ state: "Completed, Errored", exception: "Connection refused" }))
    ).toMatchObject({
      text: "Errored: Connection refused",
      tone: "danger",
      title: "Connection refused"
    });
    expect(stateChip(transfer({ state: "Completed, TimedOut" })).text).toBe("TimedOut");
    expect(stateChip(transfer({ state: "InProgress", percentComplete: 41.6 })).text).toBe("42%");
  });
});

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

  it("scope the header buttons, aborted counted as errored", () => {
    const pick = (scope: { test: (s: string) => boolean }) => STATES.filter((s) => scope.test(s));
    expect(pick(SCOPES.retry[0])).toEqual([
      "Completed, TimedOut",
      "Completed, Errored",
      "Completed, Rejected",
      "Completed, Aborted"
    ]);
    expect(pick(SCOPES.cancel[1])).toEqual(["Queued, Remotely", "Queued, Locally"]);
    expect(pick(SCOPES.remove[3])).toHaveLength(6);
  });

  it("count the active transfers a cancel asks about and sum up a result", () => {
    expect(activeAmong(rows)).toBe(1);
    expect(summary("remove", { ok: ["a", "b"], failed: [{ id: "c", error: "gone" }] })).toBe(
      "2 removed, 1 failed: gone"
    );
  });
});

describe("the transfers view", () => {
  it("filters by user, folder, extension and words", () => {
    const keep = textFilter("user:ali folder:album ext:flac one")!;
    expect(keep(transfer())).toBe(true);
    expect(keep(transfer({ username: "bob" }))).toBe(false);
    expect(keep(transfer({ filename: "@@a\\Music\\Album\\01 One.mp3" }))).toBe(false);
    expect(textFilter("  ")).toBeNull();
  });

  it("groups by user then folder, and folds a group to its header", () => {
    const a = transfer({ filename: "x\\A\\1.flac" });
    const b = transfer({ filename: "x\\B\\1.flac" });
    const c = transfer({ username: "bob", filename: "y\\1.flac" });
    const lines = (folded: Set<string>) =>
      layout([a, b, c], "user", folded).map((i) => (i.type === "group" ? i.group.label : i.key));
    expect(lines(new Set())).toEqual(["alice", "x\\A", a.id, "x\\B", b.id, "bob", "y", c.id]);
    expect(lines(new Set(["user:alice"]))).toEqual(["alice", "bob", "y", c.id]);
    expect(layout([a, c], "flat", new Set()).map((i) => i.key)).toEqual([a.id, c.id]);
  });
});

describe("the add dialog's paste box", () => {
  it("reads slsk links and the JSON of files or search responses", () => {
    expect([...parsePaste("slsk://al%20ice/Music/Album/01.flac\nnot a link")]).toEqual([
      ["al ice", [{ filename: "Music\\Album\\01.flac", size: 0 }]]
    ]);
    expect([
      ...parsePaste(
        JSON.stringify([
          { username: "bob", filename: "a\\1.mp3", size: 5 },
          { username: "bob", files: [{ filename: "a\\2.mp3", size: 7 }], lockedFiles: [] }
        ])
      )
    ]).toEqual([
      [
        "bob",
        [
          { filename: "a\\1.mp3", size: 5 },
          { filename: "a\\2.mp3", size: 7 }
        ]
      ]
    ]);
    expect(parsePaste("{ broken").size).toBe(0);
  });
});
