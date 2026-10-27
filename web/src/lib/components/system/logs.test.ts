import { describe, expect, it } from "vitest";

import { logLine, numbered } from "./logs";

const record = (message: string, timestamp = "2026-10-26T10:00:00Z") =>
  ({ timestamp, context: "reseek::x", subContext: null, level: "Information", message }) as const;

describe("live log lines", () => {
  it("keeps the lines of records read again, alike ones in order, and numbers new ones", () => {
    const shown = [logLine(record("b")), logLine(record("a")), logLine(record("a"))];
    const again = numbered([record("a"), record("a"), record("b"), record("c")], shown);
    expect(again.slice(0, 3)).toEqual([shown[2], shown[1], shown[0]]);
    expect(again[3].seq).toBeGreaterThan(shown[2].seq);
  });
});
