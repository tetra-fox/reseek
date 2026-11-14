import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { fakeNetwork } from "#lib/testing/network.js";

import { downloadFolderPlan, downloadPlan, enqueueJob, enqueueSelection } from "./downloads";

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
  ).map((g): [string | null, string[]] => [g.folder, g.files.map((f) => f.filename)]);

describe("downloading a selection", () => {
  it("sends it in the deepest folder holding it, its own files first, each folder's together", () => {
    expect(
      plan(
        [
          "C:\\Music\\Album\\Disc 1\\Extras\\x.mp3",
          "C:\\Music\\Album\\Disc 1\\01.mp3",
          "C:\\Music\\Album\\cover.jpg",
          "C:\\Music\\Album\\Disc 1\\Extras\\y.mp3"
        ],
        () => "C:"
      )
    ).toEqual([
      [
        "C:\\Music\\Album",
        [
          "C:\\Music\\Album\\cover.jpg",
          "C:\\Music\\Album\\Disc 1\\Extras\\x.mp3",
          "C:\\Music\\Album\\Disc 1\\Extras\\y.mp3",
          "C:\\Music\\Album\\Disc 1\\01.mp3"
        ]
      ]
    ]);
    expect(plan(["top/sub/a", "top/sub/b"], () => "top", "/")).toEqual([
      ["top/sub", ["top/sub/a", "top/sub/b"]]
    ]);
  });

  it("sends each top folder's files as a group of their own", () => {
    const top = (path: string) => path.split("\\").slice(0, 2).join("\\");
    expect(
      plan(["@@a\\Music\\A\\1.flac", "@@a\\Video\\x.mkv", "@@a\\Music\\A\\CD2\\2.flac"], top)
    ).toEqual([
      ["@@a\\Music\\A", ["@@a\\Music\\A\\1.flac", "@@a\\Music\\A\\CD2\\2.flac"]],
      ["@@a\\Video", ["@@a\\Video\\x.mkv"]]
    ]);
  });

  it("sends files that are in no folder in none", () => {
    expect(plan(["a.flac", "b.flac"], () => "")).toEqual([[null, ["a.flac", "b.flac"]]]);
    // a file at a peer's root is below no folder but the root
    expect(plan(["\\x.flac"], () => "\\")).toEqual([[null, ["\\x.flac"]]]);
    expect(plan(["/x.flac"], () => "/", "/")).toEqual([[null, ["/x.flac"]]]);
    expect(plan(["/x.flac", "/home/y.flac"], () => "/", "/")).toEqual([
      [null, ["/x.flac", "/home/y.flac"]]
    ]);
  });
});

describe("downloading a selection of several users", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("keeps each user's folders below the deepest one holding their files in each share", async () => {
    const net = fakeNetwork(() => ({ id: "j", groups: [] }));
    await enqueueSelection(
      [
        { username: "alice", file: file("@@a\\Music\\Album\\CD1\\01.flac") },
        { username: "bob", file: file("/home/bob/Album/01.flac") },
        { username: "alice", file: file("@@a\\Music\\Album\\CD2\\01.flac") },
        { username: "alice", file: file("@@b\\Video\\x.mkv") }
      ],
      "s1"
    );
    const body = net.requests[0].body as {
      groups: { username: string; folder: string | null; files: unknown[] }[];
    };
    expect(body.groups.map((g) => [g.username, g.folder, g.files.length])).toEqual([
      ["alice", "@@a\\Music\\Album", 2],
      ["alice", "@@b\\Video", 1],
      ["bob", "/home/bob/Album", 1]
    ]);
  });
});

describe("downloading a folder", () => {
  it("sends a peer's root as the folder of the files below it", () => {
    const root = (name: string, files: string[], separator: "\\" | "/") =>
      downloadFolderPlan(
        name,
        files.map((f) => file(f)),
        separator
      ).map((g) => g.folder);
    expect(root("/", ["/x.flac", "/home/y.flac"], "/")).toEqual(["/"]);
    expect(root("\\", ["\\x.flac"], "\\")).toEqual(["\\"]);
  });

  const folder = (name: string, files: string[]) =>
    downloadFolderPlan(
      name,
      files.map((f) => file(f)),
      "\\"
    ).map((g): [string | null, string[]] => [g.folder, g.files.map((f) => f.filename)]);

  it("sends every file in the folder, its own files first, each folder's below together", () => {
    expect(
      folder("@@a\\Artist", [
        "@@a\\Artist\\Album\\CD1\\01.flac",
        "@@a\\Artist\\Album\\cover.jpg",
        "@@a\\Artist\\notes.txt",
        "@@a\\Artist\\Album\\CD1\\02.flac"
      ])
    ).toEqual([
      [
        "@@a\\Artist",
        [
          "@@a\\Artist\\notes.txt",
          "@@a\\Artist\\Album\\CD1\\01.flac",
          "@@a\\Artist\\Album\\CD1\\02.flac",
          "@@a\\Artist\\Album\\cover.jpg"
        ]
      ]
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
