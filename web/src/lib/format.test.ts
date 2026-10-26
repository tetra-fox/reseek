import { describe, expect, it } from "vitest";

import {
  formatAttributes,
  formatBytes,
  formatBytesAsUnit,
  formatDuration,
  formatPercent,
  formatSeconds,
  formatSpeed,
  formatSpeedPadded,
  formatWait,
  getDirectoryName,
  getFileName,
  truncate
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
  });

  it("formats what isn't a size as nothing", () => {
    expect(formatBytes(Number.NaN)).toBe("0 B");
    expect(formatBytes(-5)).toBe("0 B");
    expect(formatBytes(Infinity)).toBe("0 B");
  });

  it("formats one unit", () => {
    expect(formatBytesAsUnit(1234567, "MB", 2)).toBe(1.18);
    expect(formatBytesAsUnit(12, "B")).toBe("12 B");
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
    expect(formatSeconds(0)).toBe("00:00");
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

  it("formats percentages", () => {
    expect(formatPercent(0.256)).toBe("25.6%");
    expect(formatPercent(Number.NaN)).toBe("0.0%");
  });
});

describe("files", () => {
  it("formats attributes by what the file is", () => {
    expect(formatAttributes({ bitDepth: 16, sampleRate: 44100 })).toBe("16/44.1kHz");
    expect(formatAttributes({ bitDepth: 24, sampleRate: 96000, bitRate: 4608 })).toBe("24/96kHz");
    expect(formatAttributes({ bitRate: 245, isVariableBitRate: true })).toBe("245 Kbps, VBR");
    expect(formatAttributes({ isVariableBitRate: true })).toBe("VBR");
    expect(formatAttributes({ bitRate: 320 })).toBe("320 Kbps");
    expect(formatAttributes({})).toBe("");
  });

  it("splits remote paths", () => {
    expect(getFileName("Music\\Artist\\01 - Song.flac")).toBe("01 - Song.flac");
    expect(getFileName("music/artist/song.mp3")).toBe("song.mp3");
    expect(getDirectoryName("a\\b\\c.mp3")).toBe("a\\b");
    expect(getDirectoryName("a/b/c.mp3")).toBe("a/b");
    expect(getDirectoryName("\\foo")).toBe("\\foo");
    expect(getDirectoryName("c.mp3")).toBe("c.mp3");
  });

  it("truncates", () => {
    expect(truncate(null, 3)).toBe("");
    expect(truncate("abc", 3)).toBe("abc");
    expect(truncate("abcd", 3)).toBe("abc...");
  });
});
