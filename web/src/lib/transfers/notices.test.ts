import { afterEach, describe, expect, it, vi } from "vitest";

import type { Schemas } from "#lib/api/client.js";
import { notifier } from "#lib/chat/notify.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import "./notices.svelte";

type Job = Schemas["DownloadJobFinished"];

const job = (fields: Partial<Job>): Job => ({
  id: crypto.randomUUID(),
  usernames: ["alice"],
  filename: "@@a\\Music\\Album\\01 One.flac",
  folder: "@@a\\Music\\Album",
  fileCount: 1,
  succeededCount: 1,
  failedCount: 0,
  canceledCount: 0,
  ...fields
});

describe("transfer notifications", () => {
  afterEach(() => {
    live.stop();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("tell of each finished job, a job of one file as a download", async () => {
    const told = vi.spyOn(notifier, "other").mockImplementation(() => {});
    const net = fakeNetwork(() => undefined);
    live.start();
    net.frame("hello", { principal: { name: "a", role: "ReadOnly" } } as Schemas["Hello"]);
    const finished = (fields: Partial<Job>) => net.frame("transfers.job_finished", job(fields));
    finished({});
    finished({ succeededCount: 0, failedCount: 1 });
    finished({ fileCount: 12, succeededCount: 9, failedCount: 2, canceledCount: 1 });
    finished({ usernames: ["alice", "bob", "carol"], folder: "", fileCount: 3, succeededCount: 3 });
    finished({ folder: "@@a", fileCount: 2, succeededCount: 2 });
    finished({ fileCount: 3, succeededCount: 0, failedCount: 3 });
    finished({ folder: "", fileCount: 2, succeededCount: 0, failedCount: 1, canceledCount: 1 });
    finished({ fileCount: 2, succeededCount: 0, canceledCount: 2 });
    for (let i = 0; i < 5; i++) await new Promise((done) => setTimeout(done, 0));
    expect(told.mock.calls.map(([mode, title, body]) => [mode, title, body])).toEqual([
      ["both", "Download finished", "01 One.flac from alice"],
      ["both", "Download failed", "01 One.flac from alice"],
      ["both", "Album finished", "12 files from alice, 2 failed, 1 canceled"],
      ["both", "Downloads finished", "3 files from alice and 2 others"],
      // the top of a share is not named, nothing succeeded reads as failed, and a job canceled
      // whole is not told of
      ["both", "Downloads finished", "2 files from alice"],
      ["both", "Album failed", "3 files from alice, 3 failed"],
      ["both", "Downloads failed", "2 files from alice, 1 failed, 1 canceled"]
    ]);
  });
});
