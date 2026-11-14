import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { downloadFolderPlan, downloadPlan, enqueueJob } from "./downloads";

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
  it("splits it from the deepest folder holding it: its own files, then one group per folder below", () => {
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
      ["Album", 1],
      ["Album/Disc 1/Extras", 1],
      ["Album/Disc 1", 1],
      ["Album/Disc 2", 1]
    ]);
    // the files of one folder, with nothing below it, go where the pattern puts them
    expect(plan(["top/sub/a", "top/sub/b"], () => "top", "/")).toEqual([
      [null, ["top/sub/a", "top/sub/b"]]
    ]);
  });

  it("names every top's folder once anything in the selection is nested, so no pattern splits it", () => {
    const top = (path: string) => path.split("\\").slice(0, 2).join("\\");
    expect(
      plan(["@@a\\Music\\A\\1.flac", "@@a\\Video\\x.mkv", "@@a\\Music\\A\\CD2\\2.flac"], top)
    ).toEqual([
      ["A", ["@@a\\Music\\A\\1.flac"]],
      ["A/CD2", ["@@a\\Music\\A\\CD2\\2.flac"]],
      ["Video", ["@@a\\Video\\x.mkv"]]
    ]);
    // with nothing nested anywhere, every top's files go where the pattern puts them
    expect(plan(["@@a\\Music\\A\\1.flac", "@@a\\Video\\x.mkv"], top)).toEqual([
      [null, ["@@a\\Music\\A\\1.flac"]],
      [null, ["@@a\\Video\\x.mkv"]]
    ]);
  });
});

describe("downloading a folder", () => {
  const folder = (name: string, files: string[]) =>
    downloadFolderPlan(
      name,
      files.map((f) => file(f)),
      "\\"
    ).map((g): [string | null, string[]] => [g.destination, g.files.map((f) => f.filename)]);

  it("keeps each subfolder under the folder's name, so two discs' files never meet", () => {
    expect(folder("@@a\\Album", ["@@a\\Album\\CD1\\01.flac", "@@a\\Album\\CD2\\01.flac"])).toEqual([
      ["Album/CD1", ["@@a\\Album\\CD1\\01.flac"]],
      ["Album/CD2", ["@@a\\Album\\CD2\\01.flac"]]
    ]);
    // a folder holding one subfolder still names itself, unlike a selection of that subfolder
    expect(folder("@@a\\Album", ["@@a\\Album\\cover.jpg", "@@a\\Album\\CD1\\01.flac"])).toEqual([
      ["Album", ["@@a\\Album\\cover.jpg"]],
      ["Album/CD1", ["@@a\\Album\\CD1\\01.flac"]]
    ]);
  });

  it("puts its own files under its name beside its subfolders", () => {
    expect(
      folder("@@a\\Artist", [
        "@@a\\Artist\\notes.txt",
        "@@a\\Artist\\Album\\cover.jpg",
        "@@a\\Artist\\Album\\CD1\\01.flac"
      ])
    ).toEqual([
      ["Artist", ["@@a\\Artist\\notes.txt"]],
      ["Artist/Album", ["@@a\\Artist\\Album\\cover.jpg"]],
      ["Artist/Album/CD1", ["@@a\\Artist\\Album\\CD1\\01.flac"]]
    ]);
  });

  it("names itself when nothing is below it, so a folder download never follows the pattern", () => {
    expect(folder("@@a\\Album", ["@@a\\Album\\01.flac", "@@a\\Album\\02.flac"])).toEqual([
      ["Album", ["@@a\\Album\\01.flac", "@@a\\Album\\02.flac"]]
    ]);
  });

  it("keeps folders below its subfolders too", () => {
    expect(
      folder("@@a\\Artist", [
        "@@a\\Artist\\Album\\CD1\\01.flac",
        "@@a\\Artist\\Album\\CD2\\01.flac"
      ])
    ).toEqual([
      ["Artist/Album/CD1", ["@@a\\Artist\\Album\\CD1\\01.flac"]],
      ["Artist/Album/CD2", ["@@a\\Artist\\Album\\CD2\\01.flac"]]
    ]);
  });
});

describe("enqueueing a job", () => {
  afterEach(() => vi.unstubAllGlobals());

  /** Enqueues a job whose groups the daemon answers with `answers`, in order */
  const outcome = (answers: Omit<Schemas["EnqueuedDownloadGroup"], "username" | "enqueued">[]) => {
    fakeNetwork(() => ({
      id: "j",
      groups: answers.map((a) => ({ username: "", enqueued: [], ...a }))
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

  it("counts a group refused whole as every one of its files not queued", async () => {
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
