import { describe, expect, it } from "vitest";

import { layout, type Message } from "./messages";

const message = (localId: number, timestamp: string, author = "alice"): Message => ({
  localId,
  timestamp,
  author,
  self: false,
  text: `m${localId}`,
  mention: false,
  hidden: false,
  replayed: false,
  unread: false
});

const kinds = (lines: ReturnType<typeof layout>) =>
  lines.map((l) => (l.kind === "message" ? `${l.message.localId}${l.head ? "*" : ""}` : l.kind));

describe("the message list's lines", () => {
  it("groups one author's messages within five minutes under one header", () => {
    const lines = layout(
      [
        message(1, "2026-10-26T10:00:00Z"),
        message(2, "2026-10-26T10:04:00Z"),
        message(3, "2026-10-26T10:10:00Z"),
        message(4, "2026-10-26T10:10:30Z", "bob")
      ],
      [],
      null
    );
    expect(kinds(lines)).toEqual(["day", "1*", "2", "3*", "4*"]);
  });

  it("places a gap in time order, starts a new group after it, and an open gap last", () => {
    const lines = layout(
      [message(1, "2026-10-26T10:00:00Z"), message(2, "2026-10-26T10:01:00Z")],
      [
        { from: "2026-10-26T10:02:00Z", to: null, reason: "disconnected" },
        { from: "2026-10-26T10:00:30Z", to: "2026-10-26T10:00:50Z", reason: "daemonDown" }
      ],
      null
    );
    expect(kinds(lines)).toEqual(["day", "1*", "gap", "2*", "gap"]);
    expect(lines[2]).toMatchObject({ kind: "gap", gap: { reason: "daemonDown" } });
  });

  it("marks where the unread messages start and a day change", () => {
    const lines = layout(
      [
        message(1, "2026-10-25T23:59:00Z"),
        message(2, "2026-10-26T12:00:00Z"),
        message(3, "2026-10-26T12:01:00Z")
      ],
      [],
      3
    );
    expect(kinds(lines)).toEqual(["day", "1*", "day", "2*", "unread", "3*"]);
  });

  it("places presence lines in time order, breaking a group, and later ones last", () => {
    const burst = (key: string, at: string) => ({
      key,
      at,
      kind: "joined" as const,
      names: ["bob"]
    });
    const lines = layout(
      [message(1, "2026-10-26T10:00:00Z"), message(2, "2026-10-26T10:01:00Z")],
      [],
      null,
      [burst("p2", "2026-10-26T10:05:00Z"), burst("p1", "2026-10-26T10:00:30Z")]
    );
    expect(kinds(lines)).toEqual(["day", "1*", "presence", "2*", "presence"]);
    expect(lines.map((l) => l.key)).toEqual(["day:1", "m:1", "p1", "m:2", "p2"]);
  });
});
