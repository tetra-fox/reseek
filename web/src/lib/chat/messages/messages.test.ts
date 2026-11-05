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
  automatic: false,
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
        { from: "2026-10-26T10:02:00Z", to: null },
        { from: "2026-10-26T10:00:30Z", to: "2026-10-26T10:00:50Z" }
      ],
      null
    );
    expect(kinds(lines)).toEqual(["day", "1*", "gap", "2*", "gap"]);
    expect(lines[2]).toMatchObject({ kind: "gap", gap: { from: "2026-10-26T10:00:30Z" } });
  });

  it("marks where the unread messages start and a day change", () => {
    // days change at the browser's midnight, so these times are on its clock
    const local = (day: number, hour: number, minute: number) =>
      new Date(2026, 9, day, hour, minute).toISOString();
    const lines = layout(
      [message(1, local(25, 23, 59)), message(2, local(26, 12, 0)), message(3, local(26, 12, 1))],
      [],
      3
    );
    expect(kinds(lines)).toEqual(["day", "1*", "day", "2*", "unread", "3*"]);
  });
});
