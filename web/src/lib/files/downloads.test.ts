import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { downloadPlan, enqueueJob } from "./downloads";

const file = (filename: string, size = 10): Schemas["File"] => ({
  filename,
  size,
  code: 1,
  extension: "",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

const plan = (files: string[], topOf: (path: string) => string, separator: "\\" | "/" = "\\") =>
  downloadPlan(
    files.map((f) => file(f)),
    topOf,
    separator
  ).map((g): [string | null, string[]] => [g.destination, g.files.map((f) => f.filename)]);

describe("downloading a selection", () => {
  it("splits it from the deepest folder holding it: its own files, then one batch per subfolder", () => {
    const groups = plan(
      [
        "C:\\Music\\Album\\Disc 1\\Extras\\x.mp3",
        "C:\\Music\\Album\\cover.jpg",
        "C:\\Music\\Album\\Disc 1\\01.mp3",
        "C:\\Music\\Album\\Disc 2\\01.mp3"
      ],
      () => "C:"
    );
    expect(groups.map(([destination, files]) => [destination, files.length])).toEqual([
      [null, 1],
      ["Album/Disc 1", 2],
      ["Album/Disc 2", 1]
    ]);
    // the files of one folder go where the pattern puts them
    expect(plan(["top/sub/a", "top/sub/b"], () => "top", "/")).toEqual([
      [null, ["top/sub/a", "top/sub/b"]]
    ]);
  });

  it("splits a selection across two top folders apart, each from its own deepest folder", () => {
    const top = (path: string) => path.split("\\").slice(0, 2).join("\\");
    expect(
      plan(["@@a\\Music\\A\\1.flac", "@@a\\Video\\x.mkv", "@@a\\Music\\A\\CD2\\2.flac"], top)
    ).toEqual([
      [null, ["@@a\\Music\\A\\1.flac"]],
      ["A/CD2", ["@@a\\Music\\A\\CD2\\2.flac"]],
      [null, ["@@a\\Video\\x.mkv"]]
    ]);
  });
});

describe("enqueueing a job", () => {
  afterEach(() => vi.unstubAllGlobals());

  /** Enqueues a job whose batches the daemon answers with `answers`, in order */
  const outcome = (answers: Omit<Schemas["EnqueuedJobBatch"], "username" | "batchId">[]) => {
    fakeNetwork(() => ({
      id: "j",
      batches: answers.map((a) => ({ username: "", batchId: a.error ? null : "b", ...a }))
    }));
    return enqueueJob(
      answers.map((_, i) => ({
        username: `user${i}`,
        files: [
          { filename: `${i}\\a.flac`, size: 1 },
          { filename: `${i}\\b.flac`, size: 1 }
        ]
      }))
    );
  };
  const offline = { failures: [], error: { error: "User x appears to be offline" } };

  it("counts a batch refused whole as every one of its files not queued", async () => {
    const refusedOne = { message: "Skipped: Already in progress", filename: "1\\a.flac" };
    expect(await outcome([offline, { failures: [refusedOne], error: null }])).toEqual({
      kind: "partial",
      text: "Queued 1 of 4 files. Could not queue 2 files from user0: User x appears to be offline; 1\\a.flac: Skipped: Already in progress"
    });
    expect(await outcome([offline, offline])).toEqual({
      kind: "failed",
      text: "User x appears to be offline"
    });
    expect(await outcome([{ failures: [], error: null }])).toEqual({
      kind: "done",
      text: "2 files queued"
    });
  });
});
