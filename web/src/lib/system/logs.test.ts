import { describe, expect, it } from "vitest";

import { isShown, logLine, logTime, numbered } from "./logs";

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

describe("logTime", () => {
  // local times, as the browser shows them
  const now = new Date(2026, 10, 7, 12, 0, 0).getTime();
  const at = (...parts: [number, number, number, number, number, number]) =>
    new Date(...parts).toISOString();

  it("is the time alone today, and starts with the day before it", () => {
    expect(logTime(at(2026, 10, 7, 0, 0, 5), now)).toBe("00:00:05");
    expect(logTime(at(2026, 10, 6, 23, 59, 59), now)).toBe("Nov 6 23:59:59");
    expect(logTime(at(2025, 10, 7, 9, 5, 1), now)).toBe("Nov 7, 2025 09:05:01");
  });
});

describe("a record told again", () => {
  it("is found among the lines a read showed, and a new one is not", () => {
    const shown = [logLine(record("b")), logLine(record("a"))];
    expect(isShown(record("a"), shown)).toBe(true);
    expect(isShown(record("a", "2026-10-26T10:00:00.000000001Z"), shown)).toBe(false);
    expect(isShown(record("c"), shown)).toBe(false);
  });
});
