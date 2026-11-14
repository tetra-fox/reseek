import { describe, expect, it } from "vitest";

import {
  ago,
  dateTime,
  formatAttributes,
  formatBytes,
  formatBytesTransferred,
  formatDuration,
  formatSeconds,
  formatSpeed,
  formatSpeedPadded,
  formatWait,
  parseDuration,
  sayDuration,
  getDirectoryName,
  getFileName
} from "./format";

describe("sizes and speeds", () => {
  it("formats bytes in binary units with trailing zeros dropped", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(0.5)).toBe("0 B");
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1234567)).toBe("1.18 MB");
    expect(formatBytes(1024 ** 3)).toBe("1 GB");
    // the unit follows the rounding
    expect(formatBytes(1023.999 * 1024 ** 3)).toBe("1 TB");
    expect(formatBytes(1023.97 * 1024 ** 3, 1)).toBe("1 TB");
  });

  it("formats bytes moved of a size in the size's unit", () => {
    expect(formatBytesTransferred(10.5 * 1024 ** 2, 3.2 * 1024 ** 2)).toBe("3.2/10.5 MB");
    expect(formatBytesTransferred(0, 0)).toBe("0 B/0 B");
    expect(formatBytesTransferred(500, 20)).toBe("20/500 B");
    expect(formatBytesTransferred(2 * 1024 ** 3, 1024 ** 2)).toBe("0.0/2.0 GB");
  });

  it("formats what isn't a size as nothing", () => {
    expect(formatBytes(Number.NaN)).toBe("0 B");
    expect(formatBytes(-5)).toBe("0 B");
    expect(formatBytes(Infinity)).toBe("0 B");
  });

  it("formats speeds, padded to one width for the footer", () => {
    expect(formatSpeed(0)).toBe("0 B/s");
    expect(formatSpeed(null)).toBe("0 B/s");
    expect(formatSpeed(1572864)).toBe("1.5 MB/s");
    expect(formatSpeedPadded(0)).toBe("   0 B/s");
    expect(formatSpeedPadded(0.4)).toBe("   0 B/s");
    expect(formatSpeedPadded(1572864)).toBe(" 1.5 MB/s");
    expect(formatSpeedPadded(12.3 * 1024)).toBe("12.3 KB/s");
    expect(formatSpeedPadded(1023 * 1024)).toBe("1023 KB/s");
  });
});

describe("times", () => {
  it("formats track lengths without wrapping at a day", () => {
    expect(formatSeconds(undefined)).toBe("");
    expect(formatSeconds(null)).toBe("");
    expect(formatSeconds(-1)).toBe("");
    expect(formatSeconds(0)).toBe("");
    expect(formatSeconds(1)).toBe("00:01");
    expect(formatSeconds(65)).toBe("01:05");
    expect(formatSeconds(3599)).toBe("59:59");
    expect(formatSeconds(3600)).toBe("1:00:00");
    expect(formatSeconds(26 * 3600 + 5 * 60 + 9)).toBe("26:05:09");
  });

  it("formats waits in seconds then minutes", () => {
    expect(formatWait(0)).toBe("0s");
    expect(formatWait(42.4)).toBe("42s");
    expect(formatWait(90)).toBe("1.5m");
    expect(formatWait(3600)).toBe("60.0m");
  });

  it("formats durations in their two largest units, days included", () => {
    expect(formatDuration(0)).toBe("0s");
    expect(formatDuration(59)).toBe("59s");
    expect(formatDuration(5400)).toBe("1h 30m");
    expect(formatDuration(90061)).toBe("1d 1h");
    expect(formatDuration(86400)).toBe("1d");
  });
});

describe("files", () => {
  it("formats attributes by what the file is", () => {
    expect(formatAttributes({ bitDepth: 16, sampleRate: 44100 })).toBe("16/44.1kHz");
    expect(formatAttributes({ bitDepth: 24, sampleRate: 96000, bitRate: 4608 })).toBe("24/96kHz");
    expect(formatAttributes({ bitRate: 245, isVariableBitRate: true })).toBe("245 kbps, VBR");
    expect(formatAttributes({ isVariableBitRate: true })).toBe("VBR");
    expect(formatAttributes({ bitRate: 320 })).toBe("320 kbps");
    expect(formatAttributes({})).toBe("");
    expect(formatAttributes({ bitRate: 0 })).toBe("");
  });

  it("splits remote paths", () => {
    expect(getFileName("Music\\Artist\\01 - Song.flac")).toBe("01 - Song.flac");
    expect(getFileName("music/artist/song.mp3")).toBe("song.mp3");
    expect(getDirectoryName("a\\b\\c.mp3")).toBe("a\\b");
    expect(getDirectoryName("a/b/c.mp3")).toBe("a/b");
    expect(getDirectoryName("\\foo")).toBe("\\foo");
    expect(getDirectoryName("c.mp3")).toBe("");
  });
});

describe("instants", () => {
  const now = Date.parse("2026-11-07T12:00:00Z");
  const at = (days: number) => new Date(now - days * 86_400_000).toISOString();

  it("says a recent instant relative to now and an older one as its date", () => {
    expect(ago(at(3), now)).toBe("3 days ago");
    expect(ago(at(0), now)).toBe("just now");
    expect(ago(at(-2), now)).toBe("in 2 days");
    expect(ago(at(30), now)).toBe(
      new Date(at(30)).toLocaleDateString([], { month: "short", day: "numeric" })
    );
    expect(ago(at(3 * 365), now)).toContain("2023");
  });

  it("writes the year of an instant from another year", () => {
    expect(dateTime(at(1), now)).not.toContain("2026");
    expect(dateTime(at(3 * 365), now)).toContain("2023");
  });
});

describe("durations from options", () => {
  it("reads the units options are written in", () => {
    expect(parseDuration("2m")).toBe(120_000);
    expect(parseDuration("1h 30m")).toBe(5_400_000);
    expect(parseDuration("30s")).toBe(30_000);
    expect(parseDuration("2 weeks")).toBe(14 * 86_400_000);
    expect(parseDuration("5months 27days 19h 12m")).toBe(
      5 * 2_630_016_000 + 27 * 86_400_000 + 19 * 3_600_000 + 12 * 60_000
    );
    expect(parseDuration("2 fortnights")).toBeNull();
    expect(parseDuration(5)).toBeNull();
  });

  it("says a duration in its largest unit, rounded with about", () => {
    expect(sayDuration(30 * 86_400_000)).toBe("30 days");
    expect(sayDuration(3_600_000)).toBe("1 hour");
    expect(sayDuration(parseDuration("5months 27days 19h 12m")!)).toBe("about 6 months");
    expect(sayDuration(90_000)).toBe("about 2 minutes");
  });
});
